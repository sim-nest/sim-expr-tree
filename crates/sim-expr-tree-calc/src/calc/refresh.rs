use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    sync::Arc,
};

use sim_expr_tree_core::MountEpoch;
use sim_table_core::TablePath;

use super::*;

/// One explicit observation of a mounted backend that lacks a watch contract.
///
/// Listing and stamp keys are canonical absolute expression-tree paths. Their
/// numeric values are backend-owned monotone or content-derived stamps; the
/// calculator compares them for equality and never interprets their magnitude.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendRefreshSample {
    /// Backend-wide generation.
    pub epoch: MountEpoch,
    /// Directory listing stamps by absolute directory path.
    pub listings: BTreeMap<String, u64>,
    /// Entry stamps by absolute entry path.
    pub stamps: BTreeMap<String, u64>,
}

impl BackendRefreshSample {
    /// Creates an epoch-only backend observation.
    #[must_use]
    pub fn new(epoch: MountEpoch) -> Self {
        Self {
            epoch,
            listings: BTreeMap::new(),
            stamps: BTreeMap::new(),
        }
    }

    /// Adds a directory listing stamp.
    #[must_use]
    pub fn with_listing(mut self, path: impl Into<String>, stamp: u64) -> Self {
        self.listings.insert(path.into(), stamp);
        self
    }

    /// Adds an entry stamp.
    #[must_use]
    pub fn with_stamp(mut self, path: impl Into<String>, stamp: u64) -> Self {
        self.stamps.insert(path.into(), stamp);
        self
    }
}

/// Explicit sampling contract for one mounted backend.
///
/// Backends with a native watch contract return `true` from
/// [`Self::has_watch_contract`] and are skipped by [`ExprTreeCalc::refresh`].
/// `sample` is called only by that explicit refresh operation.
pub trait MountRefreshSource: Send + Sync {
    /// Whether this backend already reports mutations through a watch contract.
    fn has_watch_contract(&self) -> bool {
        false
    }

    /// Samples the backend's epoch, relevant listings, and entry stamps.
    fn sample(&self) -> Result<BackendRefreshSample, String>;
}

/// Evidence from one explicit refresh pass.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RefreshReport {
    /// Mount paths actually sampled.
    pub sampled_mounts: Vec<String>,
    /// Watch-managed mount paths deliberately not polled.
    pub watch_managed_mounts: Vec<String>,
    /// Mount paths whose backend-wide epoch changed.
    pub changed_epochs: Vec<String>,
    /// Absolute directory paths whose listing stamp changed.
    pub changed_listings: Vec<String>,
    /// Absolute entry paths whose stamp changed.
    pub changed_stamps: Vec<String>,
    /// Number of distinct incremental observation keys invalidated.
    pub invalidated_observations: usize,
}

/// A refresh registration, sample, or validation failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RefreshError {
    /// A sampler was attached to a path that is not mounted.
    UnknownMount {
        /// Canonical supplied mount path.
        path: String,
    },
    /// A backend failed while explicitly sampled.
    Sample {
        /// Canonical mount path.
        path: String,
        /// Stable backend diagnostic.
        message: String,
    },
    /// A backend returned a listing or stamp outside its mount.
    InvalidSamplePath {
        /// Canonical mount path.
        mount: String,
        /// Rejected sample path.
        path: String,
    },
}

impl fmt::Display for RefreshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownMount { path } => write!(f, "no mounted backend at {path}"),
            Self::Sample { path, message } => {
                write!(f, "cannot refresh mounted backend {path}: {message}")
            }
            Self::InvalidSamplePath { mount, path } => {
                write!(f, "refresh sample path {path:?} is outside mount {mount}")
            }
        }
    }
}

impl Error for RefreshError {}

impl ExprTreeCalc {
    /// Attaches an explicit refresh sampler to an existing mount.
    ///
    /// Registration does not sample the backend. The source remains idle until
    /// [`Self::refresh`] is called.
    pub fn attach_refresh_source(
        &mut self,
        path: &TablePath,
        source: Arc<dyn MountRefreshSource>,
    ) -> Result<(), RefreshError> {
        let key = path_key(path);
        if !self
            .state
            .read()
            .expect("calc state poisoned")
            .mounts
            .contains_key(&key)
        {
            return Err(RefreshError::UnknownMount { path: key });
        }
        self.refresh_sources.insert(key, source);
        Ok(())
    }

    /// Detaches and returns an explicit refresh sampler.
    pub fn detach_refresh_source(
        &mut self,
        path: &TablePath,
    ) -> Option<Arc<dyn MountRefreshSource>> {
        self.refresh_sources.remove(&path_key(path))
    }

    /// Explicitly samples every non-watch mount and invalidates changed inputs.
    ///
    /// Sampling completes before graph or control state is mutated. If any
    /// backend fails, the refresh is rejected atomically and no observation is
    /// advanced.
    pub fn refresh(&mut self) -> Result<RefreshReport, RefreshError> {
        let mut report = RefreshReport::default();
        let mut sampled = Vec::new();
        for (mount, source) in &self.refresh_sources {
            if source.has_watch_contract() {
                report.watch_managed_mounts.push(mount.clone());
                continue;
            }
            let sample = source.sample().map_err(|message| RefreshError::Sample {
                path: mount.clone(),
                message,
            })?;
            validate_sample_paths(mount, &sample)?;
            report.sampled_mounts.push(mount.clone());
            sampled.push((mount.clone(), sample));
        }

        let mut invalidated = BTreeSet::new();
        let mut control_changed = false;
        for (mount, sample) in sampled {
            let previous = self
                .refresh_samples
                .get(&mount)
                .cloned()
                .unwrap_or_else(|| BackendRefreshSample::new(MountEpoch::default()));
            if previous.epoch != sample.epoch {
                report.changed_epochs.push(mount.clone());
                invalidated.insert(CalcQuery::MountEpoch(mount.clone()));
            }
            for path in changed_paths(&previous.listings, &sample.listings) {
                report.changed_listings.push(path.clone());
                invalidated.insert(CalcQuery::Listing(path));
            }
            for path in changed_paths(&previous.stamps, &sample.stamps) {
                report.changed_stamps.push(path.clone());
                invalidated.insert(CalcQuery::LookupStep(path.clone()));
                invalidated.insert(CalcQuery::NameSlot(path.clone()));
                invalidated.insert(CalcQuery::Cell(path));
            }
            if previous != sample {
                control_changed = true;
                if let Some(state) = self
                    .state
                    .write()
                    .expect("calc state poisoned")
                    .mounts
                    .get_mut(&mount)
                {
                    state.epoch = sample.epoch;
                }
                self.refresh_samples.insert(mount, sample);
            }
        }
        if control_changed {
            let mut state = self.state.write().expect("calc state poisoned");
            bump_generation(&mut state.control_generation);
        }
        for query in &invalidated {
            self.engine.invalidate(query);
        }
        report.invalidated_observations = invalidated.len();
        if !invalidated.is_empty() {
            self.schedule_dirty_automatic();
        }
        Ok(report)
    }
}

fn changed_paths(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| before.get(*path) != after.get(*path))
        .cloned()
        .collect()
}

fn validate_sample_paths(mount: &str, sample: &BackendRefreshSample) -> Result<(), RefreshError> {
    let mount_path =
        TablePath::parse_absolute(mount).map_err(|_| RefreshError::InvalidSamplePath {
            mount: mount.to_owned(),
            path: mount.to_owned(),
        })?;
    for path in sample.listings.keys().chain(sample.stamps.keys()) {
        let parsed =
            TablePath::parse_absolute(path).map_err(|_| RefreshError::InvalidSamplePath {
                mount: mount.to_owned(),
                path: path.clone(),
            })?;
        if !is_path_prefix(&mount_path, &parsed) {
            return Err(RefreshError::InvalidSamplePath {
                mount: mount.to_owned(),
                path: path.clone(),
            });
        }
    }
    Ok(())
}

fn is_path_prefix(candidate: &TablePath, path: &TablePath) -> bool {
    candidate.segments().len() <= path.segments().len()
        && candidate
            .segments()
            .iter()
            .zip(path.segments())
            .all(|(left, right)| left == right)
}
