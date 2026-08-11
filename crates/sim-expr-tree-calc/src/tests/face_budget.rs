use std::sync::Arc;

use sim_codec::{DecodePosition, Input};
use sim_kernel::{
    Args, Callable, Cx, EncodePosition, Expr, Object, ObjectCompat, ReadPolicy, Symbol,
};

use crate::{
    CodecPolicyPatch, EncodedFace, FaceBudget, FaceContent, FaceDimension, FaceIssue, FacePosition,
};

use super::{
    codec_context, lisp_codec_calc,
    support::{path, strict_context},
};

// conformance: codec-backed source and result faces enforce independent hard budgets.

fn face_text(face: &EncodedFace) -> &str {
    match face.content() {
        Some(FaceContent::Text(text)) => text,
        other => panic!("expected text face, got {other:?}"),
    }
}

#[test]
fn face_budget_source_and_result_faces_are_position_aware_and_independent() {
    let mut calc = lisp_codec_calc(false);
    calc.set_tree_codec_policy(CodecPolicyPatch {
        source_codec: Some(Some("codec/lisp".to_owned())),
        source_position: Some(DecodePosition::Eval),
        source_budget: Some(FaceBudget::new(4_096, 32, 128)),
        result_codec: Some(Some("codec/lisp".to_owned())),
        result_position: Some(EncodePosition::Data),
        result_budget: Some(FaceBudget::new(4, 32, 128)),
        ..CodecPolicyPatch::default()
    });

    calc.set_cell(
        path("/position"),
        Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::new("f"))),
            args: vec![Expr::String("x".to_owned())],
        },
    );
    let eval_face = calc.source_face(&path("/position"));
    assert_eq!(eval_face.metadata().position(), FacePosition::Eval);
    assert!(eval_face.metadata().is_complete());
    calc.set_cell_codec_policy(
        path("/position"),
        CodecPolicyPatch {
            source_position: Some(DecodePosition::Data),
            ..CodecPolicyPatch::default()
        },
    );
    let data_face = calc.source_face(&path("/position"));
    assert_eq!(data_face.metadata().position(), FacePosition::Data);
    assert_ne!(
        face_text(&eval_face),
        face_text(&data_face),
        "the installed codec must receive the explicit output position"
    );

    calc.set_cell(path("/value"), Expr::String("bounded-result".to_owned()));
    calc.verify_cell(&path("/value")).unwrap();
    assert!(
        calc.source_face(&path("/value")).metadata().is_complete(),
        "the source face has its own larger byte budget"
    );
    assert!(matches!(
        calc.result_face(&path("/value")).metadata().issue(),
        FaceIssue::Truncated {
            dimension: FaceDimension::Bytes,
            limit: 4,
            ..
        }
    ));

    calc.set_cell_codec_policy(
        path("/huge"),
        CodecPolicyPatch {
            source_budget: Some(FaceBudget::new(8, 32, 128)),
            ..CodecPolicyPatch::default()
        },
    );
    calc.set_cell(path("/huge"), Expr::String("x".repeat(4_096)));
    assert!(matches!(
        calc.source_face(&path("/huge")).metadata().issue(),
        FaceIssue::Truncated {
            dimension: FaceDimension::Bytes,
            limit: 8,
            ..
        }
    ));

    let mut nested = Expr::Nil;
    for _ in 0..40 {
        nested = Expr::List(vec![nested]);
    }
    calc.set_cell_codec_policy(
        path("/nested"),
        CodecPolicyPatch {
            source_budget: Some(FaceBudget::new(4_096, 6, 128)),
            ..CodecPolicyPatch::default()
        },
    );
    calc.set_cell(path("/nested"), nested);
    assert!(matches!(
        calc.source_face(&path("/nested")).metadata().issue(),
        FaceIssue::Truncated {
            dimension: FaceDimension::Depth,
            limit: 6,
            ..
        }
    ));

    calc.set_cell_codec_policy(
        path("/items"),
        CodecPolicyPatch {
            source_budget: Some(FaceBudget::new(4_096, 32, 3)),
            ..CodecPolicyPatch::default()
        },
    );
    calc.set_cell(
        path("/items"),
        Expr::Vector(vec![Expr::Nil, Expr::Nil, Expr::Nil, Expr::Nil, Expr::Nil]),
    );
    assert!(matches!(
        calc.source_face(&path("/items")).metadata().issue(),
        FaceIssue::Truncated {
            dimension: FaceDimension::Items,
            limit: 3,
            ..
        }
    ));
}

struct FaceCallable;

impl Object for FaceCallable {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        panic!("bounded result faces must never stringify callables")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for FaceCallable {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for FaceCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> sim_kernel::Result<sim_kernel::Value> {
        cx.factory().nil()
    }
}

#[test]
fn face_budget_callable_results_remain_available_without_stringification() {
    let mut calc = lisp_codec_calc(false);
    calc.set_tree_codec_policy(CodecPolicyPatch {
        result_codec: Some(Some("codec/lisp".to_owned())),
        result_position: Some(EncodePosition::Data),
        result_budget: Some(FaceBudget::new(1_024, 16, 64)),
        ..CodecPolicyPatch::default()
    });
    let callable = codec_context(false)
        .factory()
        .opaque(Arc::new(FaceCallable))
        .unwrap();
    calc.bind_value(Symbol::new("face-callable"), callable);
    calc.set_cell(
        path("/callable"),
        Expr::Symbol(Symbol::new("face-callable")),
    );
    let value = calc.verify_cell(&path("/callable")).unwrap();
    assert!(value.object().as_callable().is_some());
    let face = calc.result_face(&path("/callable"));
    assert!(face.content().is_none());
    assert!(matches!(
        face.metadata().issue(),
        FaceIssue::Unsupported { reason } if reason.contains("callable")
    ));
    assert!(
        calc.current_cell(&path("/callable"))
            .unwrap()
            .object()
            .as_callable()
            .is_some(),
        "presentation failure must not replace the arbitrary calculation value"
    );
}

#[test]
fn face_budget_codec_removal_replacement_and_general_codec_round_trips() {
    let mut calc = lisp_codec_calc(false);
    let data = Expr::Vector(vec![
        Expr::String("round-trip".to_owned()),
        Expr::Bool(true),
        Expr::String("through-general-codecs".to_owned()),
    ]);
    calc.set_tree_codec_policy(CodecPolicyPatch {
        source_codec: Some(Some("codec/lisp".to_owned())),
        source_position: Some(DecodePosition::Data),
        source_budget: Some(FaceBudget::new(16_384, 64, 512)),
        result_codec: Some(Some("codec/lisp".to_owned())),
        result_position: Some(EncodePosition::Data),
        result_budget: Some(FaceBudget::new(16_384, 64, 512)),
        ..CodecPolicyPatch::default()
    });
    calc.set_cell(path("/lisp"), data.clone());
    let lisp_text = face_text(&calc.source_face(&path("/lisp"))).to_owned();
    assert!(
        calc.edit_cell_source(
            path("/lisp-copy"),
            Input::Text(lisp_text),
            ReadPolicy::default(),
        )
        .applied()
    );
    assert_eq!(
        face_text(&calc.source_face(&path("/lisp-copy"))),
        face_text(&calc.source_face(&path("/lisp"))),
        "Lisp source must survive an encode/decode/encode round trip"
    );

    for cell in ["/json", "/json-copy"] {
        calc.set_cell_codec_policy(
            path(cell),
            CodecPolicyPatch {
                source_codec: Some(Some("codec/json".to_owned())),
                result_codec: Some(Some("codec/json".to_owned())),
                ..CodecPolicyPatch::default()
            },
        );
    }
    calc.set_cell(path("/json"), data);
    let json_text = face_text(&calc.source_face(&path("/json"))).to_owned();
    assert!(
        calc.edit_cell_source(
            path("/json-copy"),
            Input::Text(json_text),
            ReadPolicy::default(),
        )
        .applied()
    );
    assert_eq!(
        face_text(&calc.source_face(&path("/json-copy"))),
        face_text(&calc.source_face(&path("/json"))),
        "JSON source must survive an encode/decode/encode round trip"
    );

    calc.set_cell(path("/replace"), Expr::String("codec-choice".to_owned()));
    calc.verify_cell(&path("/replace")).unwrap();
    let lisp = face_text(&calc.result_face(&path("/replace"))).to_owned();
    calc.set_cell_codec_policy(
        path("/replace"),
        CodecPolicyPatch {
            result_codec: Some(Some("codec/json".to_owned())),
            ..CodecPolicyPatch::default()
        },
    );
    let json = face_text(&calc.result_face(&path("/replace"))).to_owned();
    assert_ne!(
        lisp, json,
        "replacing the selected installed codec takes effect"
    );

    calc.replace_context_factory(strict_context);
    calc.set_codec_registry_revision(2);
    assert!(matches!(
        calc.result_face(&path("/replace")).metadata().issue(),
        FaceIssue::CodecFailure { message } if message.contains("unknown symbol")
    ));
}
