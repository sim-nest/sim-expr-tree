use std::{
    fmt,
    sync::{Arc, RwLock},
};

use super::*;

pub(super) fn expr_identity(expr: &Expr) -> u64 {
    let mut identity = StableIdentity::new();
    identity.write_debug(&expr.canonical_key());
    identity.finish()
}

pub(super) fn state_identities(
    state: &Arc<RwLock<CalcState>>,
    context_factory: &Arc<ContextFactory>,
) -> Result<(u64, u64), DerivedSnapshotError> {
    let (
        cells,
        bound_names,
        bound_values,
        codec_registry_revision,
        tree_calc_policy,
        dir_calc_policies,
        cell_calc_policies,
        tree_codec_policy,
        dir_codec_policies,
        cell_codec_policies,
        authority_ceiling,
        tree_authority_policy,
        dir_authority_policies,
        cell_authority_policies,
        mounts,
    ) = {
        let state = state.read().expect("calc state poisoned");
        (
            state.cells.clone(),
            state.bound_names.clone(),
            state.bound_values.clone(),
            state.codec_registry_revision,
            state.tree_calc_policy.clone(),
            state.dir_calc_policies.clone(),
            state.cell_calc_policies.clone(),
            state.tree_codec_policy.clone(),
            state.dir_codec_policies.clone(),
            state.cell_codec_policies.clone(),
            state.authority_ceiling.clone(),
            state.tree_authority_policy.clone(),
            state.dir_authority_policies.clone(),
            state.cell_authority_policies.clone(),
            state.mounts.clone(),
        )
    };
    let mut source = StableIdentity::new();
    source.write_debug(&cells);
    source.write_debug(&bound_names);
    let mut cx = context_factory();
    for (name, value) in &bound_values {
        source.write(name.to_string().as_bytes());
        let expr = value.object().as_expr(&mut cx).map_err(backend_error)?;
        source.write_debug(&expr.canonical_key());
    }
    let mut control = StableIdentity::new();
    control.write_debug(&codec_registry_revision);
    control.write_debug(&tree_calc_policy);
    control.write_debug(&dir_calc_policies);
    control.write_debug(&cell_calc_policies);
    control.write_debug(&tree_codec_policy);
    control.write_debug(&dir_codec_policies);
    control.write_debug(&cell_codec_policies);
    control.write_debug(&authority_ceiling);
    control.write_debug(&tree_authority_policy);
    control.write_debug(&dir_authority_policies);
    control.write_debug(&cell_authority_policies);
    control.write_debug(&mounts);
    Ok((source.finish(), control.finish()))
}

pub(super) struct StableIdentity(u64);

impl StableIdentity {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    pub(super) fn new() -> Self {
        Self(Self::OFFSET)
    }

    pub(super) fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
        self.0 ^= 0xff;
        self.0 = self.0.wrapping_mul(Self::PRIME);
    }

    pub(super) fn write_debug(&mut self, value: &impl fmt::Debug) {
        self.write(format!("{value:?}").as_bytes());
    }

    pub(super) fn finish(self) -> u64 {
        self.0
    }
}
