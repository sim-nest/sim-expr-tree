//! conformance: finite expression-tree namespace records.

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
        PolicyPatch::empty(),
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
fn policy_effective_resolution_inherits_overrides_and_clears() {
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
        PolicyPatch::clear_codec(),
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
        .with_policy_patch(PolicyPatch::set_codec(CodecPolicy::new("codec:lisp"))),
    )
    .unwrap();

    assert_eq!(
        ns.effective_dir_policy(&folder_id)
            .unwrap()
            .codec()
            .unwrap()
            .codec(),
        "codec:bridge"
    );
    assert_eq!(ns.effective_dir_policy(&cleared_id).unwrap().codec(), None);
    assert_eq!(
        ns.effective_cell_policy(&cell)
            .unwrap()
            .codec()
            .unwrap()
            .codec(),
        "codec:lisp"
    );
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
