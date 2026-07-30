//! conformance: finite expression-tree namespace records.

use std::collections::BTreeMap;

use sim_table_core::{TablePath, TablePathRef};

use super::*;

fn ids() -> (TreeId, DirId) {
    (TreeId::new("tree-a").unwrap(), DirId::new("root").unwrap())
}

fn namespace() -> Namespace {
    let (tree, root) = ids();
    Namespace::new(tree, root)
}

fn named(value: &str) -> NamespaceName {
    NamespaceName::new(value).unwrap()
}

#[test]
fn name_explicit_naming_rejects_collisions_and_illegal_segments() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let name = ns.reserve_name(lane, &root, named("total")).unwrap();
    ns.create_cell(
        lane,
        CellCreate::new(
            CellId::new("cell-a").unwrap(),
            root.clone(),
            name,
            NodeKind::Source,
        ),
    )
    .unwrap();

    assert_eq!(
        ns.reserve_name(lane, &root, named("total")),
        Err(NamespaceError::NameCollision {
            parent: root.clone(),
            name: named("total")
        })
    );
    assert_eq!(
        NamespaceName::new("../escape"),
        Err(NamespaceError::IllegalName("../escape".to_owned()))
    );
}

#[test]
fn name_generated_reservations_survive_crash_gaps() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let gap = ns
        .reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    assert_eq!(gap.as_str(), "cell-1");

    let created = ns
        .reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    assert_eq!(created.as_str(), "cell-2");
    ns.create_cell(
        lane,
        CellCreate::new(
            CellId::new("cell-b").unwrap(),
            root.clone(),
            created,
            NodeKind::Derived,
        ),
    )
    .unwrap();

    assert_eq!(ns.reservation_count(), 1);
    assert!(ns.child(&root, &named("cell-1")).is_none());
    assert_eq!(
        ns.child(&root, &named("cell-2")),
        Some(NamespaceEntry::Cell {
            id: CellId::new("cell-b").unwrap(),
            kind: NodeKind::Derived
        })
    );
}

#[test]
fn name_counter_corruption_is_detected() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let name = ns
        .reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    ns.create_cell(
        lane,
        CellCreate::new(
            CellId::new("cell-a").unwrap(),
            root.clone(),
            name,
            NodeKind::Source,
        ),
    )
    .unwrap();

    ns.set_counter_for_test(root.clone(), GeneratedNameKind::Cell, 0);
    assert_eq!(
        ns.reserve_generated_name(lane, &root, GeneratedNameKind::Cell),
        Err(NamespaceError::CounterCorruption {
            parent: root,
            kind: GeneratedNameKind::Cell,
            candidate: named("cell-1")
        })
    );
}

#[test]
fn name_serialized_writer_lane_rejects_second_writer() {
    let mut ns = namespace();
    let lane = ns.acquire_writer().unwrap();
    assert_eq!(
        ns.acquire_writer(),
        Err(NamespaceError::WriterAlreadyActive)
    );
    ns.release_writer(lane).unwrap();
    assert!(ns.acquire_writer().is_ok());
}

#[test]
fn name_concurrent_requests_are_ordered_by_one_writer_lane() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let first = ns
        .reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    let second = ns
        .reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    assert_eq!(first.as_str(), "cell-1");
    assert_eq!(second.as_str(), "cell-2");
}

#[test]
fn name_rename_and_move_preserve_identity() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let work = ns.reserve_name(lane, &root, named("work")).unwrap();
    ns.create_dir(
        lane,
        DirId::new("dir-work").unwrap(),
        &root,
        work,
        CodecPolicyPatch::empty(),
    )
    .unwrap();
    let cell_name = ns.reserve_name(lane, &root, named("source")).unwrap();
    let cell = CellId::new("cell-source").unwrap();
    ns.create_cell(
        lane,
        CellCreate::new(cell.clone(), root.clone(), cell_name, NodeKind::Source),
    )
    .unwrap();

    let work_dir = DirId::new("dir-work").unwrap();
    ns.move_cell(lane, &cell, &work_dir, named("renamed"))
        .unwrap();
    assert_eq!(ns.cell(&cell).unwrap().id(), &cell);
    assert_eq!(ns.cell(&cell).unwrap().parent(), &work_dir);
    assert_eq!(
        ns.child(&work_dir, &named("renamed")).unwrap(),
        NamespaceEntry::Cell {
            id: cell,
            kind: NodeKind::Source,
        }
    );

    ns.move_dir(lane, &work_dir, &root, named("moved")).unwrap();
    assert_eq!(ns.dir(&work_dir).unwrap().id(), &work_dir);
    assert_eq!(
        ns.child(&root, &named("moved")).unwrap(),
        NamespaceEntry::Dir { id: work_dir }
    );
}

#[test]
fn name_finite_reads_do_not_allocate_namespace_state() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    ns.reserve_generated_name(lane, &root, GeneratedNameKind::Cell)
        .unwrap();
    let reservations = ns.reservation_count();
    let counters = ns.counter_count();

    assert!(ns.cell(&CellId::new("missing").unwrap()).is_none());
    assert!(ns.child(&root, &named("missing")).is_none());
    assert_eq!(ns.reservation_count(), reservations);
    assert_eq!(ns.counter_count(), counters);
}

#[test]
fn codec_policy_effective_resolution_inherits_overrides_and_clears() {
    let mut ns = namespace();
    let root = ns.root_dir().clone();
    let lane = ns.acquire_writer().unwrap();
    let folder = ns.reserve_name(lane, &root, named("folder")).unwrap();
    let folder_id = DirId::new("folder-dir").unwrap();
    ns.create_dir(
        lane,
        folder_id.clone(),
        &root,
        folder,
        Namespace::codec_patch("codec:bridge"),
    )
    .unwrap();
    let cleared = ns.reserve_name(lane, &folder_id, named("plain")).unwrap();
    let cleared_id = DirId::new("plain-dir").unwrap();
    ns.create_dir(
        lane,
        cleared_id.clone(),
        &folder_id,
        cleared,
        CodecPolicyPatch::clear_codec(),
    )
    .unwrap();
    let cell_name = ns.reserve_name(lane, &folder_id, named("cell")).unwrap();
    let cell = CellId::new("cell-policy").unwrap();
    ns.create_cell(
        lane,
        CellCreate::new(
            cell.clone(),
            folder_id.clone(),
            cell_name,
            NodeKind::Control,
        )
        .with_policy_patch(CodecPolicyPatch::set_codec("codec:lisp")),
    )
    .unwrap();

    assert_eq!(
        ns.effective_dir_policy(&folder_id)
            .unwrap()
            .source_codec()
            .unwrap(),
        "codec:bridge"
    );
    assert_eq!(
        ns.effective_dir_policy(&cleared_id).unwrap().source_codec(),
        None
    );
    assert_eq!(
        ns.effective_cell_policy(&cell)
            .unwrap()
            .source_codec()
            .unwrap(),
        "codec:lisp"
    );
}

#[test]
fn codec_policy_inherits_each_source_and_result_field_independently() {
    use sim_codec::{DecodeLimits, DecodePosition};
    use sim_kernel::EncodePosition;

    let inherited = EffectiveCodecPolicy::derive([
        CodecPolicyPatch {
            source_codec: Some(Some("codec/lisp".to_owned())),
            source_position: Some(DecodePosition::Quote),
            decode_limits: Some(DecodeLimits {
                max_input_bytes: 4_096,
                ..DecodeLimits::default()
            }),
            source_budget: Some(FaceBudget::new(1_024, 12, 80)),
            result_codec: Some(Some("codec/json".to_owned())),
            result_position: Some(EncodePosition::Data),
            result_budget: Some(FaceBudget::new(2_048, 16, 120)),
        },
        CodecPolicyPatch {
            source_codec: Some(Some("codec/algol".to_owned())),
            result_position: Some(EncodePosition::Quote),
            ..CodecPolicyPatch::default()
        },
    ]);

    assert_eq!(inherited.source_codec(), Some("codec/algol"));
    assert_eq!(inherited.source_position(), DecodePosition::Quote);
    assert_eq!(inherited.decode_limits().max_input_bytes, 4_096);
    assert_eq!(inherited.source_budget(), FaceBudget::new(1_024, 12, 80));
    assert_eq!(inherited.result_codec(), Some("codec/json"));
    assert_eq!(inherited.result_position(), EncodePosition::Quote);
    assert_eq!(inherited.result_budget(), FaceBudget::new(2_048, 16, 120));

    let clamped = EffectiveCodecPolicy::derive([CodecPolicyPatch {
        decode_limits: Some(DecodeLimits {
            max_input_bytes: usize::MAX,
            max_tokens: usize::MAX,
            max_expr_nodes: usize::MAX,
            max_depth: usize::MAX,
            max_string_bytes: usize::MAX,
            max_blob_bytes: usize::MAX,
            max_collection_len: usize::MAX,
            max_trivia_items: usize::MAX,
        }),
        source_budget: Some(FaceBudget::new(usize::MAX, usize::MAX, usize::MAX)),
        ..CodecPolicyPatch::default()
    }]);
    assert_eq!(clamped.decode_limits(), DecodeLimits::default());
    assert_eq!(clamped.source_budget().max_bytes(), HARD_MAX_FACE_BYTES);
    assert_eq!(clamped.source_budget().max_depth(), HARD_MAX_FACE_DEPTH);
    assert_eq!(clamped.source_budget().max_items(), HARD_MAX_FACE_ITEMS);
}

#[test]
fn policy_records_source_stamps_and_reuses_table_path_references() {
    let stamp = Stamp::new(RevisionTick::new(7, 11), Some(WallTimeMs::new(1234)));
    let source = SourceRecord::new("import:csv", stamp);
    assert_eq!(source.origin(), "import:csv");
    assert_eq!(
        source.observed_at().wall_time_ms().unwrap().unix_millis(),
        1234
    );

    let base = TablePath::from_segments(["sheet", "row"]).unwrap();
    let reference = TablePathRef::parse("../total").unwrap();
    let resolved = resolve_namespace_path(&base, &reference).unwrap();
    assert_eq!(resolved.to_string(), "/sheet/total");
}

fn absolute(path: &str) -> TablePath {
    TablePath::parse_absolute(path).unwrap()
}

#[test]
fn store_typed_adapters_keep_source_control_and_derived_lanes_separate() {
    let root = DirId::new("root").unwrap();
    let mut stores = ExprTreeStores::new(root.clone()).unwrap();
    let cell = CellId::new("source-cell").unwrap();
    let mut source = BTreeMap::new();
    source.insert(
        cell.clone(),
        SourceEntry::new("(+ a b)").with_codec("codec:lisp"),
    );
    let mut control = BTreeMap::new();
    control.insert("counter:/".to_owned(), ControlEntry::Counter(3));
    let mut pending = ExprTreeStores::prepare_source_control_commit(source, control);

    stores.recover_commit(&mut pending);
    stores.put_derived(cell.clone(), DerivedEntry::CachedValue("42".to_owned()));
    stores.put_control("ui:expanded", ControlEntry::UiPreference("true".to_owned()));

    assert_eq!(stores.root_dir(), &root);
    assert_eq!(stores.source_entry(&cell).unwrap().expr(), "(+ a b)");
    assert_eq!(
        stores.source_entry(&cell).unwrap().codec(),
        Some("codec:lisp")
    );
    assert_eq!(
        stores.control_entry("counter:/"),
        Some(&ControlEntry::Counter(3))
    );
    assert_eq!(
        stores.derived_entry(&cell),
        Some(&DerivedEntry::CachedValue("42".to_owned()))
    );
    assert_eq!(crate::store::source_keys_for_test(&stores).len(), 1);
}

#[test]
fn store_source_control_commit_recovers_from_partial_failure() {
    let mut stores = ExprTreeStores::new(DirId::new("root").unwrap()).unwrap();
    let cell = CellId::new("source-cell").unwrap();
    let mut source = BTreeMap::new();
    source.insert(cell.clone(), SourceEntry::new("(* subtotal tax)"));
    let mut control = BTreeMap::new();
    control.insert(
        "policy:/sheet".to_owned(),
        ControlEntry::Policy(EffectiveCodecPolicy::empty()),
    );
    let mut pending = ExprTreeStores::prepare_source_control_commit(source, control);

    stores.commit_source(&mut pending);
    assert!(pending.source_committed());
    assert!(!pending.control_committed());
    assert!(stores.source_entry(&cell).is_some());
    assert!(stores.control_entry("policy:/sheet").is_none());

    stores.recover_commit(&mut pending);
    assert!(pending.control_committed());
    assert!(stores.control_entry("policy:/sheet").is_some());
}

#[test]
fn store_reopen_validates_filesystem_database_and_read_only_mounts() {
    let mounts = vec![
        MountDescriptor::dir(
            absolute("/files"),
            BackendKind::Filesystem,
            MountEpoch::new(10),
        ),
        MountDescriptor::dir(absolute("/db"), BackendKind::Database, MountEpoch::new(11)),
        MountDescriptor::table(
            absolute("/catalog"),
            BackendKind::ReadOnly,
            MountEpoch::new(12),
        ),
    ];

    let stores = ExprTreeStores::reopen(
        DirId::new("root").unwrap(),
        BTreeMap::new(),
        BTreeMap::new(),
        BTreeMap::new(),
        mounts,
    )
    .unwrap();

    let observed: Vec<_> = stores
        .mounts()
        .map(|mount| (mount.path().to_string(), mount.resource(), mount.backend()))
        .collect();
    assert_eq!(
        observed,
        vec![
            (
                "/catalog".to_owned(),
                MountResource::Table,
                BackendKind::ReadOnly
            ),
            ("/db".to_owned(), MountResource::Dir, BackendKind::Database),
            (
                "/files".to_owned(),
                MountResource::Dir,
                BackendKind::Filesystem
            ),
        ]
    );
}

#[test]
fn mount_requires_explicit_operation_and_never_flattens_table_leaves() {
    let mut stores = ExprTreeStores::new(DirId::new("root").unwrap()).unwrap();
    assert_eq!(
        stores.return_value_without_mounting(MountResource::Table),
        0
    );

    stores
        .mount(MountDescriptor::table(
            absolute("/results"),
            BackendKind::Memory,
            MountEpoch::new(1),
        ))
        .unwrap();
    assert_eq!(stores.mounts().count(), 1);
    assert_eq!(
        stores.mount(MountDescriptor::dir(
            absolute("/results/detail"),
            BackendKind::Database,
            MountEpoch::new(1),
        )),
        Err(StoreError::TableMountIsLeaf(absolute("/results")))
    );
}

#[test]
fn mount_epochs_are_control_state_and_survive_reopen() {
    let mut stores = ExprTreeStores::new(DirId::new("root").unwrap()).unwrap();
    stores
        .mount(MountDescriptor::dir(
            absolute("/remote"),
            BackendKind::MountedNamespace,
            MountEpoch::new(1),
        ))
        .unwrap();
    stores
        .observe_mount_epoch(&absolute("/remote"), MountEpoch::new(2))
        .unwrap();

    assert_eq!(
        stores.control_entry("mount-epoch:/remote"),
        Some(&ControlEntry::MountEpoch(MountEpoch::new(2)))
    );
    assert_eq!(stores.mounts().next().unwrap().epoch(), MountEpoch::new(2));
}

#[test]
fn mount_corruption_and_root_mounts_fail_closed() {
    let mut stores = ExprTreeStores::new(DirId::new("root").unwrap()).unwrap();
    assert_eq!(
        stores.mount(MountDescriptor::dir(
            TablePath::root(),
            BackendKind::Memory,
            MountEpoch::new(0),
        )),
        Err(StoreError::InvalidMount(
            "root is supplied as the required root Dir, not as a mount".to_owned()
        ))
    );
    assert_eq!(
        stores.observe_mount_epoch(&absolute("/missing"), MountEpoch::new(2)),
        Err(StoreError::CorruptMount(
            "missing mount /missing".to_owned()
        ))
    );
}
