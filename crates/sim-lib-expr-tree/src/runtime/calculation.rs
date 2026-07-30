use std::sync::Arc;

use sim_expr_tree_calc::{
    AutomaticBudget, CalcExplanation, CalcPolicyPatch, CalcRequestMode, CalcStatus,
    CodecPolicyPatch, DirectedCalcReport, RefreshReport, RequestId,
};
use sim_expr_tree_core::ControlEntry;
use sim_kernel::Value;
use sim_lib_stream_core::{BufferPolicy, StreamValue};
use sim_table_core::TablePath;

use super::{EntryIdentity, TreeState, resolve_path};
use crate::TreeCellInspection;
use crate::{DurablePolicyRecord, DurableSourceRecord, runtime_support::trigger_name};

impl TreeState {
    pub(crate) fn set_wall_clock<F>(&mut self, clock: F)
    where
        F: Fn() -> Option<u64> + Send + Sync + 'static,
    {
        self.calc.set_wall_clock(clock);
    }

    pub(crate) fn inspect_cell(
        &self,
        path: &str,
    ) -> std::result::Result<TreeCellInspection, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let cell = self.cell(&path)?;
        let explanation = self.calc.explain(&path);
        let policy = self.calc.effective_calc_policy(&path);
        let codec = self.calc.effective_codec_policy(&path);
        let mut policy_badges = vec![trigger_name(policy.trigger).to_owned()];
        if let Some(source_codec) = codec.source_codec() {
            policy_badges.push(format!("source:{source_codec}"));
        }
        if let Some(result_codec) = codec.result_codec() {
            policy_badges.push(format!("result:{result_codec}"));
        }
        Ok(TreeCellInspection {
            path: path.to_absolute_reference(),
            source: self.calc.source_face(&path),
            result: self.calc.result_face(&path),
            status: explanation.status,
            source_revision: cell.revision,
            receipt: explanation.receipt,
            policy_badges,
        })
    }

    pub(crate) fn set_calc_policy(
        &mut self,
        path: &str,
        patch: CalcPolicyPatch,
    ) -> std::result::Result<DurablePolicyRecord, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let key = path.to_absolute_reference();
        match self.entry(&path)? {
            EntryIdentity::Dir(_) if path.is_root() => self.calc.set_tree_calc_policy(patch),
            EntryIdentity::Dir(_) => self.calc.set_dir_calc_policy(path.clone(), patch),
            EntryIdentity::Cell(_) => self.calc.set_cell_calc_policy(path.clone(), patch),
        }
        self.stores.put_control(
            format!("calc-policy:{key}"),
            ControlEntry::UiPreference(format!("{:?}", self.calc_policy(&path).trigger)),
        );
        Ok(self.policy_record(&path))
    }

    pub(crate) fn set_codec_policy(
        &mut self,
        path: &str,
        patch: CodecPolicyPatch,
    ) -> std::result::Result<DurablePolicyRecord, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let identity = self.entry(&path)?;
        match &identity {
            EntryIdentity::Dir(_) if path.is_root() => {
                self.calc.set_tree_codec_policy(patch.clone());
            }
            EntryIdentity::Dir(id) => {
                let id = id.clone();
                self.with_writer(|namespace, lane| {
                    namespace.set_dir_policy(lane, &id, patch.clone())
                })?;
                self.calc.set_dir_codec_policy(path.clone(), patch);
            }
            EntryIdentity::Cell(id) => {
                let id = id.clone();
                self.with_writer(|namespace, lane| {
                    namespace.set_cell_policy(lane, &id, patch.clone())
                })?;
                self.calc.set_cell_codec_policy(path.clone(), patch);
            }
        }
        let effective = self.calc.effective_codec_policy(&path);
        self.stores.put_control(
            format!("codec-policy:{}", path.to_absolute_reference()),
            ControlEntry::Policy(effective),
        );
        Ok(self.policy_record(&path))
    }

    pub(crate) fn calculate(
        &mut self,
        path: &str,
        mode: CalcRequestMode,
    ) -> std::result::Result<DirectedCalcReport, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.cell(&path)?;
        Ok(self.calc.calculate_cells([path], mode, Default::default()))
    }

    pub(crate) fn current_value(&self, path: &str) -> std::result::Result<Value, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.cell(&path)?;
        self.calc
            .current_cell(&path)
            .map_err(|error| error.to_string())
    }

    pub(crate) fn cancel(&mut self, request_id: u64) -> bool {
        self.calc.cancel_request(RequestId::new(request_id))
    }

    pub(crate) fn refresh(&mut self) -> std::result::Result<RefreshReport, String> {
        self.calc.refresh().map_err(|error| error.to_string())
    }

    pub(crate) fn status(&self, path: &str) -> std::result::Result<CalcStatus, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.cell(&path)?;
        Ok(self.calc.explain(&path).status)
    }

    pub(crate) fn pending_request_id(
        &self,
        path: &str,
    ) -> std::result::Result<Option<RequestId>, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.cell(&path)?;
        let key = path.to_absolute_reference();
        Ok(self
            .calc
            .automatic_queue_snapshot()
            .entries
            .into_iter()
            .find_map(|entry| (entry.cell == key).then_some(entry.request_id)))
    }

    pub(crate) fn explanation(&self, path: &str) -> std::result::Result<CalcExplanation, String> {
        let path = resolve_path(path, &TablePath::root())?;
        self.cell(&path)?;
        Ok(self.calc.explain(&path))
    }

    pub(crate) fn watch(&mut self) -> Arc<StreamValue> {
        Arc::clone(
            self.calc
                .watch(BufferPolicy::bounded(128).expect("nonzero stream bound"))
                .stream(),
        )
    }

    pub(crate) fn source_record(
        &self,
        path: &str,
    ) -> std::result::Result<DurableSourceRecord, String> {
        let path = resolve_path(path, &TablePath::root())?;
        let key = path.to_absolute_reference();
        let cell = self
            .cells
            .get(&key)
            .ok_or_else(|| format!("not a cell: {key}"))?;
        Ok(DurableSourceRecord {
            path: key,
            source: cell.source.clone(),
            revision: cell.revision,
        })
    }

    fn policy_record(&self, path: &TablePath) -> DurablePolicyRecord {
        let calc = self.calc_policy(path);
        let codec = self.calc.effective_codec_policy(path);
        DurablePolicyRecord {
            path: path.to_absolute_reference(),
            calc_trigger: trigger_name(calc.trigger).to_owned(),
            source_codec: codec.source_codec().map(str::to_owned),
            result_codec: codec.result_codec().map(str::to_owned),
        }
    }

    fn calc_policy(&self, path: &TablePath) -> sim_expr_tree_calc::EffectiveCalcPolicy {
        self.calc.effective_calc_policy(path)
    }

    pub(super) fn run_ready_automatic(&mut self) {
        self.calc
            .run_automatic(AutomaticBudget::default(), u64::MAX);
    }
}
