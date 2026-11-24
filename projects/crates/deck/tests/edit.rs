//! Integration tests for the deck editing surface.

use document_presentation::authored::{
    ChartData, ChartElementInput, ChartSeries, ChartType, Deck, DeckCreateOptions, ElementContent, Fill, ImageElementInput,
    Line, ShapeElementInput, SlideAddOptions, TableCell, TableElementInput, TableRow, TextContent, TextElementInput, Transform,
};
use open_ppt_deck::edit::{edit, slide_detail};
use open_ppt_deck::summary;
use serde_json::{Value, json};

fn two_shapes() -> (Deck, String, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut ids = (String::new(), String::new());
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            let first = edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "rect".to_string(),
                    transform: Some(Transform::from_points(10.0, 10.0, 50.0, 50.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#FF0000".to_string() }),
                    line: None,
                    alt: None,
                },
            )?;
            let second = edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "rect".to_string(),
                    transform: Some(Transform::from_points(20.0, 20.0, 50.0, 50.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#00FF00".to_string() }),
                    line: None,
                    alt: None,
                },
            )?;
            ids = (first.as_str().to_string(), second.as_str().to_string());
            Ok(slide)
        })
        .expect("shape deck builds");
    (deck, slide.as_str().to_string(), ids.0, ids.1)
}

fn background_deck() -> (Deck, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Background"),
                    transform: Some(Transform::from_points(40.0, 40.0, 400.0, 100.0).unwrap()),
                    alt: None,
                    placeholder: None,
                },
            )?;
            Ok(slide)
        })
        .expect("text deck builds");
    (deck, slide.as_str().to_string())
}

fn svg(deck: &Deck) -> String {
    let report = open_ppt_deck::render(deck).expect("render succeeds");
    report["pages"][0]["svg"].as_str().expect("svg page").to_string()
}

fn titled_deck(titles: &[&str]) -> (Deck, Vec<String>) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut ids = Vec::new();
    deck.transaction(None, |edit| {
        for title in titles {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            edit.set_slide_title(&slide, (*title).to_string())?;
            ids.push(slide.as_str().to_string());
        }
        Ok(())
    })
    .expect("titled deck builds");
    (deck, ids)
}

fn slide_titles(deck: &Deck) -> Vec<String> {
    deck.slides().iter().map(|slide| slide.title()).collect()
}

#[test]
fn slide_move_reorders_one_slide_without_touching_the_others() {
    let (mut deck, ids) = titled_deck(&["Intro", "Plan", "Close"]);
    let revision = deck.revision();
    let report = edit(&mut deck, &json!({ "operations": [{ "op": "slide.move", "slideId": ids[2], "index": 0 }] }))
        .expect("slide move commits");

    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
    assert_eq!(slide_titles(&deck), ["Close", "Intro", "Plan"]);
    assert_eq!(report["deck"]["slides"][0]["id"].as_str(), Some(ids[2].as_str()));
}

#[test]
fn slide_move_rejects_unknown_slides_and_out_of_range_indexes() {
    let (mut deck, ids) = titled_deck(&["Intro", "Plan"]);
    let revision = deck.revision();

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.move", "slideId": "slide-missing", "index": 0 }] }))
        .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.move", "slideId": ids[0], "index": 2 }] }))
        .expect_err("an out-of-range index is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed moves consume no revision");
    assert_eq!(slide_titles(&deck), ["Intro", "Plan"], "the order is unchanged");
}

#[test]
fn slide_swap_exchanges_two_slides_in_place() {
    let (mut deck, ids) = titled_deck(&["Intro", "Plan", "Close"]);
    let revision = deck.revision();
    let report = edit(&mut deck, &json!({ "operations": [{ "op": "slide.swap", "slideId": ids[0], "index": 2 }] }))
        .expect("slide swap commits");

    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
    assert_eq!(slide_titles(&deck), ["Close", "Plan", "Intro"], "only the two slides move");
    assert_eq!(report["deck"]["slides"][0]["id"].as_str(), Some(ids[2].as_str()));
    assert_eq!(report["deck"]["slides"][2]["id"].as_str(), Some(ids[0].as_str()));
    assert_eq!(report["results"][0], Value::Null, "a swap reports no created id");
}

#[test]
fn slide_swap_with_its_own_position_keeps_the_order() {
    let (mut deck, ids) = titled_deck(&["Intro", "Plan"]);
    let revision = deck.revision();
    edit(&mut deck, &json!({ "operations": [{ "op": "slide.swap", "slideId": ids[1], "index": 1 }] }))
        .expect("a same-position swap is a no-op");

    assert_eq!(deck.revision(), revision + 1, "a no-op still commits its batch");
    assert_eq!(slide_titles(&deck), ["Intro", "Plan"], "the order is unchanged");
}

#[test]
fn slide_swap_rejects_unknown_slides_and_out_of_range_indexes() {
    let (mut deck, ids) = titled_deck(&["Intro", "Plan"]);
    let revision = deck.revision();

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.swap", "slideId": "slide-missing", "index": 0 }] }))
        .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.swap", "slideId": ids[0], "index": 2 }] }))
        .expect_err("an out-of-range index is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.swap", "slideId": ids[0] }] }))
        .expect_err("a missing index is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed swaps consume no revision");
    assert_eq!(slide_titles(&deck), ["Intro", "Plan"], "the order is unchanged");
}

fn element_ids(deck: &Deck, slide: &str) -> Vec<String> {
    slide_detail(deck, slide).expect("slide exists")["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .filter_map(|element| element["id"].as_str().map(str::to_string))
        .collect()
}

#[test]
fn z_order_commands_restack_one_element() {
    for name in ["element.bringToFront", "element.bringForward"] {
        let (mut deck, slide, first, second) = two_shapes();
        let revision = deck.revision();
        let report = edit(&mut deck, &json!({ "operations": [{ "op": name, "slideId": slide, "elementId": first }] }))
            .expect("restack commits");

        assert_eq!(report["revision"], revision + 1, "{name} is one revision bump");
        assert_eq!(element_ids(&deck, &slide), [second.clone(), first.clone()], "{name} raises the first element");
        let painted = svg(&deck);
        assert!(
            painted.find("fill=\"#00FF00\"").unwrap() < painted.find("fill=\"#FF0000\"").unwrap(),
            "{name} paints the raised element last: {painted}"
        );
    }

    for name in ["element.sendToBack", "element.sendBackward"] {
        let (mut deck, slide, first, second) = two_shapes();
        edit(&mut deck, &json!({ "operations": [{ "op": name, "slideId": slide, "elementId": second }] }))
            .expect("restack commits");

        assert_eq!(element_ids(&deck, &slide), [second.clone(), first.clone()], "{name} lowers the second element");
    }
}

#[test]
fn z_order_rejects_unknown_slides_and_elements() {
    let (mut deck, slide, first, second) = two_shapes();
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.bringToFront", "slideId": slide, "elementId": "element-missing" }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.sendToBack", "slideId": "slide-missing", "elementId": first }],
        }),
    )
    .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    assert_eq!(deck.revision(), revision, "failed restacks consume no revision");
    assert_eq!(element_ids(&deck, &slide), [first, second], "the order is unchanged");
}

#[test]
fn element_reorder_changes_the_paint_order() {
    let (mut deck, slide, first, second) = two_shapes();
    let before = svg(&deck);
    assert!(
        before.find("fill=\"#FF0000\"").unwrap() < before.find("fill=\"#00FF00\"").unwrap(),
        "the first element paints first: {before}"
    );

    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": second, "index": 0 }],
        }),
    )
    .expect("reorder commits");

    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
    let after = svg(&deck);
    assert!(
        after.find("fill=\"#00FF00\"").unwrap() < after.find("fill=\"#FF0000\"").unwrap(),
        "the moved element paints first: {after}"
    );

    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_eq!(detail["elements"][0]["id"], second);
    assert_eq!(detail["elements"][1]["id"], first);
}

#[test]
fn slide_background_edit_reaches_render_and_exports() {
    let (mut deck, slide) = background_deck();
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "slide.setBackground",
                "slideId": slide,
                "fill": { "kind": "solid", "color": "112233" },
            }],
        }),
    )
    .expect("background commits");

    assert_eq!(report["revision"], revision + 1);
    assert_eq!(slide_detail(&deck, &slide).expect("slide")["background"], "#112233");
    assert!(svg(&deck).contains("fill=\"#112233\""), "SVG paints the new background");

    let directory = std::env::temp_dir().join("open-ppt-background-edit");
    std::fs::create_dir_all(&directory).expect("temp directory");
    let pdf = directory.join("background.pdf");
    open_ppt_deck::export(&deck, &json!({ "format": "pdf", "target": pdf.display().to_string(), "overwrite": true }))
        .expect("pdf export");
    let pdf_text = String::from_utf8_lossy(&std::fs::read(&pdf).expect("pdf bytes")).to_string();
    assert!(pdf_text.contains("0.067 0.133 0.2 rg"), "PDF paints the new background: {pdf_text}");

    let pptx = directory.join("background.pptx");
    open_ppt_deck::export(&deck, &json!({ "format": "pptx", "target": pptx.display().to_string(), "overwrite": true }))
        .expect("pptx export");
    let pptx_text = String::from_utf8_lossy(&std::fs::read(&pptx).expect("pptx bytes")).to_string();
    assert!(pptx_text.contains("112233"), "PPTX writes the new background");
}

#[test]
fn clearing_a_slide_background_restores_the_default() {
    let (mut deck, slide) = background_deck();
    edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "slide.setBackground",
                "slideId": slide,
                "fill": { "kind": "solid", "color": "#ABCDEF" },
            }],
        }),
    )
    .expect("background commits");
    assert!(svg(&deck).contains("fill=\"#ABCDEF\""));

    let revision = deck.revision();
    let report = edit(&mut deck, &json!({ "operations": [{ "op": "slide.setBackground", "slideId": slide, "fill": null }] }))
        .expect("background clears");

    assert_eq!(report["revision"], revision + 1, "clearing is a commit too");
    assert!(slide_detail(&deck, &slide).expect("slide")["background"].is_null());
    let cleared = svg(&deck);
    assert!(!cleared.contains("#ABCDEF"), "the old background is gone");
    assert!(cleared.contains("fill=\"#FFFFFF\""), "the default background takes over");
}

#[test]
fn invalid_reorder_and_background_arguments_do_not_commit() {
    let (mut deck, slide, _, second) = two_shapes();
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": "element-missing", "index": 0 }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.reorder", "slideId": slide, "elementId": second, "index": 9 }],
        }),
    )
    .expect_err("an out-of-range index is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "slide.setBackground",
                "slideId": slide,
                "fill": { "kind": "gradient", "color": "#000000" },
            }],
        }),
    )
    .expect_err("an unknown fill kind is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "slide.setBackground",
                "slideId": slide,
                "fill": { "kind": "solid", "color": "pink" },
            }],
        }),
    )
    .expect_err("an unusable colour is refused");
    assert_eq!(error.code, "session/argument-invalid");
    assert_eq!(deck.revision(), revision, "failed scripts consume no revision");

    let report = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "element.reorder", "slideId": slide, "elementId": second, "index": 0 },
                { "op": "slide.setBackground", "slideId": slide, "fill": { "kind": "solid", "color": "#102030" } },
            ],
        }),
    )
    .expect("deferred operations commit together");
    assert_eq!(report["revision"], revision + 1, "two deferred operations still bump once");
}

fn shape_deck(boxes: &[(f64, f64, f64, f64)]) -> (Deck, String, Vec<String>) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut ids = Vec::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            for (x, y, width, height) in boxes {
                let id = edit.add_shape(
                    &slide,
                    ShapeElementInput {
                        geometry: "rect".to_string(),
                        transform: Some(Transform::from_points(*x, *y, *width, *height).unwrap()),
                        fill: None,
                        line: None,
                        alt: None,
                    },
                )?;
                ids.push(id.as_str().to_string());
            }
            Ok(slide)
        })
        .expect("shape deck builds");
    (deck, slide.as_str().to_string(), ids)
}

fn frame_of(detail: &Value, element: &str) -> (f64, f64, f64, f64) {
    let entry = detail["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .find(|candidate| candidate["id"] == element)
        .unwrap_or_else(|| panic!("element `{element}` is missing"));
    let transform = &entry["transform"];
    (
        transform["xPt"].as_f64().expect("x"),
        transform["yPt"].as_f64().expect("y"),
        transform["widthPt"].as_f64().expect("width"),
        transform["heightPt"].as_f64().expect("height"),
    )
}

fn assert_frame(detail: &Value, element: &str, expected: (f64, f64, f64, f64)) {
    let actual = frame_of(detail, element);
    let label = format!("element `{element}`: {actual:?} != {expected:?}");
    assert!((actual.0 - expected.0).abs() < 0.01, "{label}");
    assert!((actual.1 - expected.1).abs() < 0.01, "{label}");
    assert!((actual.2 - expected.2).abs() < 0.01, "{label}");
    assert!((actual.3 - expected.3).abs() < 0.01, "{label}");
}

#[test]
fn element_align_pins_all_six_edges_to_the_slide() {
    let cases = [
        ("left", (0.0, 10.0, 50.0, 50.0), (0.0, 20.0, 50.0, 50.0)),
        ("center", (455.0, 10.0, 50.0, 50.0), (455.0, 20.0, 50.0, 50.0)),
        ("right", (910.0, 10.0, 50.0, 50.0), (910.0, 20.0, 50.0, 50.0)),
        ("top", (10.0, 0.0, 50.0, 50.0), (20.0, 0.0, 50.0, 50.0)),
        ("middle", (10.0, 245.0, 50.0, 50.0), (20.0, 245.0, 50.0, 50.0)),
        ("bottom", (10.0, 490.0, 50.0, 50.0), (20.0, 490.0, 50.0, 50.0)),
    ];
    for (edge, first_expected, second_expected) in cases {
        let (mut deck, slide, first, second) = two_shapes();
        let revision = deck.revision();
        let report = edit(
            &mut deck,
            &json!({
                "operations": [{
                    "op": "element.align",
                    "slideId": slide,
                    "elementIds": [first, second],
                    "edge": edge,
                }],
            }),
        )
        .expect("alignment commits");

        assert_eq!(report["revision"], revision + 1, "{edge} is one revision bump");
        let detail = slide_detail(&deck, &slide).expect("slide exists");
        assert_frame(&detail, &first, first_expected);
        assert_frame(&detail, &second, second_expected);
    }
}

#[test]
fn element_align_uses_the_selection_bounds() {
    let (mut deck, slide, ids) = shape_deck(&[(100.0, 100.0, 50.0, 50.0), (300.0, 120.0, 100.0, 80.0)]);
    edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": ids,
                "edge": "right",
                "relativeTo": "selection",
            }],
        }),
    )
    .expect("selection alignment commits");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &ids[0], (350.0, 100.0, 50.0, 50.0));
    assert_frame(&detail, &ids[1], (300.0, 120.0, 100.0, 80.0));

    let (mut deck, slide, ids) = shape_deck(&[(100.0, 100.0, 50.0, 50.0), (300.0, 120.0, 100.0, 80.0)]);
    edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": ids,
                "edge": "bottom",
                "relativeTo": "selection",
            }],
        }),
    )
    .expect("selection alignment commits");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &ids[0], (100.0, 150.0, 50.0, 50.0));
    assert_frame(&detail, &ids[1], (300.0, 120.0, 100.0, 80.0));
}

#[test]
fn element_align_rejects_bad_arguments_without_committing() {
    let (mut deck, slide, first, second) = two_shapes();
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.align", "slideId": slide, "elementIds": [], "edge": "left" }],
        }),
    )
    .expect_err("an empty list is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": [first, first],
                "edge": "left",
            }],
        }),
    )
    .expect_err("a repeated element is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": [first, "element-missing"],
                "edge": "left",
            }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": [first, second],
                "edge": "sideways",
            }],
        }),
    )
    .expect_err("an unknown edge is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.align",
                "slideId": slide,
                "elementIds": [first, second],
                "edge": "left",
                "relativeTo": "page",
            }],
        }),
    )
    .expect_err("an unknown reference frame is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed alignments consume no revision");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &first, (10.0, 10.0, 50.0, 50.0));
    assert_frame(&detail, &second, (20.0, 20.0, 50.0, 50.0));
}

#[test]
fn element_distribute_evenly_spaces_both_axes() {
    let (mut deck, slide, ids) = shape_deck(&[
        (50.0, 10.0, 40.0, 40.0),
        (180.0, 10.0, 60.0, 40.0),
        (300.0, 10.0, 20.0, 40.0),
        (500.0, 10.0, 40.0, 40.0),
    ]);
    edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": ids,
                "axis": "horizontal",
            }],
        }),
    )
    .expect("horizontal distribution commits");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &ids[0], (50.0, 10.0, 40.0, 40.0));
    assert_frame(&detail, &ids[1], (200.0, 10.0, 60.0, 40.0));
    assert_frame(&detail, &ids[2], (370.0, 10.0, 20.0, 40.0));
    assert_frame(&detail, &ids[3], (500.0, 10.0, 40.0, 40.0));

    let (mut deck, slide, ids) = shape_deck(&[
        (10.0, 40.0, 40.0, 30.0),
        (10.0, 150.0, 40.0, 50.0),
        (10.0, 260.0, 40.0, 20.0),
        (10.0, 410.0, 40.0, 30.0),
    ]);
    edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": ids,
                "axis": "vertical",
            }],
        }),
    )
    .expect("vertical distribution commits");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &ids[0], (10.0, 40.0, 40.0, 30.0));
    assert_frame(&detail, &ids[1], (10.0, 160.0, 40.0, 50.0));
    assert_frame(&detail, &ids[2], (10.0, 300.0, 40.0, 20.0));
    assert_frame(&detail, &ids[3], (10.0, 410.0, 40.0, 30.0));
}

#[test]
fn element_distribute_rejects_bad_arguments_without_committing() {
    let (mut deck, slide, ids) = shape_deck(&[(50.0, 10.0, 40.0, 40.0), (180.0, 10.0, 60.0, 40.0), (500.0, 10.0, 40.0, 40.0)]);
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": [ids[0], ids[1]],
                "axis": "horizontal",
            }],
        }),
    )
    .expect_err("fewer than three elements is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": [ids[0], ids[1], ids[1]],
                "axis": "horizontal",
            }],
        }),
    )
    .expect_err("a repeated element is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": [ids[0], ids[1], "element-missing"],
                "axis": "horizontal",
            }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.distribute",
                "slideId": slide,
                "elementIds": ids,
                "axis": "diagonal",
            }],
        }),
    )
    .expect_err("an unknown axis is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed distributions consume no revision");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &ids[0], (50.0, 10.0, 40.0, 40.0));
    assert_frame(&detail, &ids[1], (180.0, 10.0, 60.0, 40.0));
    assert_frame(&detail, &ids[2], (500.0, 10.0, 40.0, 40.0));
}

#[test]
fn alignment_batches_commit_once_and_undo_as_a_whole() {
    let (mut deck, slide, first, second) = two_shapes();
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "element.align", "slideId": slide, "elementIds": [first, second], "edge": "left" },
                { "op": "element.align", "slideId": slide, "elementIds": [first, second], "edge": "top" },
            ],
        }),
    )
    .expect("both alignments commit together");
    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");

    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &first, (0.0, 0.0, 50.0, 50.0));
    assert_frame(&detail, &second, (0.0, 0.0, 50.0, 50.0));

    assert!(deck.undo(), "undo restores the previous document");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &first, (10.0, 10.0, 50.0, 50.0));
    assert_frame(&detail, &second, (20.0, 20.0, 50.0, 50.0));
    assert_eq!(deck.revision(), revision + 2, "undo is itself a commit");

    assert!(deck.redo(), "redo reapplies the aligned document");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &first, (0.0, 0.0, 50.0, 50.0));
    assert_frame(&detail, &second, (0.0, 0.0, 50.0, 50.0));
}

#[test]
fn a_failing_operation_rolls_back_the_whole_alignment_batch() {
    let (mut deck, slide, first, second) = two_shapes();
    let revision = deck.revision();
    let error = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "element.align", "slideId": slide, "elementIds": [first, second], "edge": "left" },
                {
                    "op": "element.distribute",
                    "slideId": slide,
                    "elementIds": [first, second, "element-missing"],
                    "axis": "horizontal",
                },
            ],
        }),
    )
    .expect_err("the second operation fails");
    assert_eq!(error.code, "session/argument-invalid");
    assert_eq!(deck.revision(), revision, "no revision is consumed");

    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_frame(&detail, &first, (10.0, 10.0, 50.0, 50.0));
    assert_frame(&detail, &second, (20.0, 20.0, 50.0, 50.0));
}

fn element_entry<'a>(detail: &'a Value, element: &str) -> &'a Value {
    detail["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .find(|candidate| candidate["id"] == element)
        .unwrap_or_else(|| panic!("element `{element}` is missing"))
}

/// One text, shape, table and image element, in paint order.
fn duplicate_deck() -> (Deck, String, Vec<String>) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut ids = Vec::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            let text = edit.add_text(
                &slide,
                TextElementInput {
                    text: TextContent::plain("Hello"),
                    transform: Some(Transform::from_points(10.0, 10.0, 120.0, 40.0).unwrap()),
                    alt: Some("greeting".to_string()),
                    placeholder: None,
                },
            )?;
            let shape = edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "roundRect".to_string(),
                    transform: Some(Transform::from_points(20.0, 90.0, 80.0, 40.0).unwrap()),
                    fill: Some(Fill::Solid { color: "#123456".to_string() }),
                    line: Some(Line { color: Some("#000000".to_string()), width_emu: Some(12_700) }),
                    alt: Some("box".to_string()),
                },
            )?;
            let table = edit.add_table(
                &slide,
                TableElementInput {
                    column_widths_emu: vec![1_270_000, 2_540_000],
                    rows: vec![TableRow { height_emu: 508_000, cells: vec![TableCell::plain("A"), TableCell::plain("B")] }],
                    transform: Some(Transform::from_points(30.0, 160.0, 300.0, 60.0).unwrap()),
                    alt: Some("grid".to_string()),
                },
            )?;
            let asset = edit.add_asset("pixel.png", "image/png", vec![0x89, b'P', b'N', b'G'], None)?;
            let image = edit.add_image(
                &slide,
                ImageElementInput {
                    asset,
                    transform: Some(Transform::from_points(40.0, 250.0, 100.0, 80.0).unwrap()),
                    alt: "diagram".to_string(),
                },
            )?;
            ids = vec![
                text.as_str().to_string(),
                shape.as_str().to_string(),
                table.as_str().to_string(),
                image.as_str().to_string(),
            ];
            Ok(slide)
        })
        .expect("duplicate fixture builds");
    (deck, slide.as_str().to_string(), ids)
}

#[test]
fn element_duplicate_copies_supported_kinds_to_the_end() {
    let (mut deck, slide, sources) = duplicate_deck();
    let before = slide_detail(&deck, &slide).expect("slide exists");
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [
                {
                    "op": "element.duplicate",
                    "slideId": slide,
                    "elementId": sources[0],
                    "offsetXPt": 10.0,
                    "offsetYPt": 20.0,
                    "as": "textCopy",
                },
                {
                    "op": "element.duplicate",
                    "slideId": slide,
                    "elementId": sources[1],
                    "offsetXPt": 10.0,
                    "offsetYPt": 20.0,
                    "as": "shapeCopy",
                },
                {
                    "op": "element.duplicate",
                    "slideId": slide,
                    "elementId": sources[2],
                    "offsetXPt": 10.0,
                    "offsetYPt": 20.0,
                    "as": "tableCopy",
                },
                {
                    "op": "element.duplicate",
                    "slideId": slide,
                    "elementId": sources[3],
                    "offsetXPt": 10.0,
                    "offsetYPt": 20.0,
                    "as": "imageCopy",
                },
            ],
        }),
    )
    .expect("duplicates commit");

    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
    let copies = ["textCopy", "shapeCopy", "tableCopy", "imageCopy"]
        .map(|token| report["resolved"][token].as_str().expect("resolved duplicate id").to_string());
    let after = slide_detail(&deck, &slide).expect("slide exists");
    for (source, copy) in sources.iter().zip(&copies) {
        assert_ne!(source, copy, "each duplicate takes a fresh id");
        let source_entry = element_entry(&before, source);
        let copy_entry = element_entry(&after, copy);
        assert_eq!(copy_entry["kind"], source_entry["kind"], "the duplicate keeps its kind");
        assert_eq!(copy_entry["content"], source_entry["content"], "the duplicate keeps its content");
        assert_eq!(copy_entry["alt"], source_entry["alt"], "the duplicate keeps its alternative text");
        if let Some(text) = source_entry.get("text") {
            assert_eq!(copy_entry["text"], *text, "the duplicate keeps its text");
        }
        let (x, y, width, height) = frame_of(&before, source);
        assert_frame(&after, copy, (x + 10.0, y + 20.0, width, height));
    }

    let painted = element_ids(&deck, &slide);
    assert_eq!(painted.len(), sources.len() + copies.len(), "four elements were appended");
    assert_eq!(&painted[sources.len()..], &copies[..], "duplicates paint last in operation order");
}

#[test]
fn element_duplicate_without_offsets_keeps_the_transform() {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut source = String::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            let mut transform = Transform::from_points(120.0, 80.0, 200.0, 60.0).unwrap();
            transform.rotation_deg = Some(15.0);
            let shape = edit.add_shape(
                &slide,
                ShapeElementInput {
                    geometry: "rect".to_string(),
                    transform: Some(transform),
                    fill: Some(Fill::Solid { color: "#ABCDEF".to_string() }),
                    line: None,
                    alt: Some("rotated".to_string()),
                },
            )?;
            source = shape.as_str().to_string();
            Ok(slide)
        })
        .expect("rotated shape builds");
    let slide = slide.as_str().to_string();

    let report = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.duplicate", "slideId": slide, "elementId": source, "as": "copy" }],
        }),
    )
    .expect("duplicate commits");
    let copy = report["resolved"]["copy"].as_str().expect("resolved duplicate id");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    let source_transform = &element_entry(&detail, &source)["transform"];
    let copy_transform = &element_entry(&detail, copy)["transform"];
    assert_eq!(copy_transform, source_transform, "an absent offset leaves position, size and rotation untouched");
    assert_eq!(copy_transform["rotationDeg"].as_f64(), Some(15.0));
}

#[test]
fn element_duplicate_can_target_another_slide() {
    let (mut deck, slide, sources) = duplicate_deck();
    let target = deck
        .transaction(None, |edit| edit.add_slide(SlideAddOptions { layout: None, index: None }))
        .expect("second slide builds")
        .as_str()
        .to_string();
    let before = element_ids(&deck, &slide).len();

    let report = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.duplicate",
                "slideId": slide,
                "elementId": sources[1],
                "targetSlideId": target,
                "offsetXPt": 5.0,
                "as": "copy",
            }],
        }),
    )
    .expect("cross-slide duplicate commits");
    let copy = report["resolved"]["copy"].as_str().expect("resolved duplicate id");

    assert_eq!(element_ids(&deck, &slide).len(), before, "the source slide keeps its elements");
    assert_eq!(element_ids(&deck, &target), vec![copy.to_string()], "the copy lands on the target slide");
    let source_detail = slide_detail(&deck, &slide).expect("source exists");
    let target_detail = slide_detail(&deck, &target).expect("target exists");
    let source_entry = element_entry(&source_detail, &sources[1]);
    let copy_entry = element_entry(&target_detail, copy);
    let source_x = source_entry["transform"]["xPt"].as_f64().expect("source x");
    let copy_x = copy_entry["transform"]["xPt"].as_f64().expect("copy x");
    assert!((copy_x - source_x - 5.0).abs() < 0.01, "the requested offset applies");
    assert_eq!(copy_entry["geometry"], source_entry["geometry"], "the shape is rebuilt faithfully");
}

#[test]
fn element_duplicate_rejects_unknown_target_slides() {
    let (mut deck, slide, sources) = duplicate_deck();
    let revision = deck.revision();
    let error = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.duplicate",
                "slideId": slide,
                "elementId": sources[0],
                "targetSlideId": "slide-missing",
            }],
        }),
    )
    .expect_err("an unknown target slide is refused");
    assert_eq!(error.code, "object/not-found");
    assert_eq!(deck.revision(), revision, "failed duplicates consume no revision");
}

#[test]
fn element_duplicate_rejects_unknown_targets_and_unsupported_kinds() {
    let (mut deck, slide, sources) = duplicate_deck();
    let revision = deck.revision();
    let count = element_ids(&deck, &slide).len();

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.duplicate", "slideId": "slide-missing", "elementId": sources[0] }],
        }),
    )
    .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.duplicate", "slideId": slide, "elementId": "element-missing" }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "object/not-found");

    assert_eq!(deck.revision(), revision, "failed duplicates consume no revision");
    assert_eq!(element_ids(&deck, &slide).len(), count, "failed duplicates append nothing");

    let (mut deck, slide, chart) = chart_deck();
    let revision = deck.revision();
    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.duplicate", "slideId": slide, "elementId": chart }],
        }),
    )
    .expect_err("charts are refused");
    assert_eq!(error.code, "object/unsupported");
    assert_eq!(deck.revision(), revision, "unsupported duplicates consume no revision");
    assert_eq!(element_ids(&deck, &slide), vec![chart], "the slide is unchanged");
}

fn chart_deck() -> (Deck, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut chart_id = String::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            let chart = edit.add_chart(
                &slide,
                ChartElementInput {
                    chart_type: ChartType::Column,
                    data: ChartData {
                        categories: vec!["A".to_string()],
                        series: vec![ChartSeries { name: "S".to_string(), values: vec![1.0] }],
                    },
                    transform: Some(Transform::from_points(10.0, 10.0, 100.0, 80.0).unwrap()),
                    alt: "chart".to_string(),
                },
            )?;
            chart_id = chart.as_str().to_string();
            Ok(slide)
        })
        .expect("chart deck builds");
    (deck, slide.as_str().to_string(), chart_id)
}

#[test]
fn element_text_style_reaches_every_run() {
    let (mut deck, slide) = background_deck();
    let element = element_ids(&deck, &slide)[0].clone();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({
            "operations": [{
                "op": "element.setTextStyle",
                "slideId": slide,
                "elementId": element,
                "bold": true,
                "sizePt": 28,
                "color": "#0a0",
                "fontFamily": "Inter",
            }],
        }),
    )
    .expect("text style commits");
    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");

    let detail = slide_detail(&deck, &slide).expect("slide detail");
    let style = element_entry(&detail, &element)["style"].clone();
    assert_eq!(style["bold"], json!(true));
    assert_eq!(style["sizePt"], json!(28.0));
    assert_eq!(style["color"], json!("#00AA00"));
    assert_eq!(style["fontFamily"], json!("Inter"));

    let error =
        edit(&mut deck, &json!({ "operations": [{ "op": "element.setTextStyle", "slideId": slide, "elementId": element }] }))
            .expect_err("an empty style is refused");
    assert_eq!(error.code, "session/argument-invalid");

    for operation in [
        json!({ "op": "element.setTextStyle", "slideId": slide, "elementId": element, "sizePt": 0 }),
        json!({ "op": "element.setTextStyle", "slideId": slide, "elementId": element, "color": "red" }),
        json!({ "op": "element.setTextStyle", "slideId": slide, "elementId": "element-missing", "bold": true }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("invalid text style");
        assert!(error.code == "session/argument-invalid" || error.code == "object/not-found");
    }
    assert_eq!(deck.revision(), revision + 1, "failed styles consume no revision");

    let (mut shapes, shape_slide, shape, _) = two_shapes();
    let error = edit(
        &mut shapes,
        &json!({ "operations": [{
            "op": "element.setTextStyle",
            "slideId": shape_slide,
            "elementId": shape,
            "bold": true,
        }] }),
    )
    .expect_err("a shape has no text runs");
    assert_eq!(error.code, "session/argument-invalid");
}

#[test]
fn element_hyperlink_stamps_and_clears_every_run() {
    let (mut deck, slide) = background_deck();
    let element = element_ids(&deck, &slide)[0].clone();
    let revision = deck.revision();

    edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "element.setHyperlink",
            "slideId": slide,
            "elementId": element,
            "url": " https://example.com/report ",
        }] }),
    )
    .expect("hyperlink commits");
    let detail = slide_detail(&deck, &slide).expect("slide detail");
    assert_eq!(element_entry(&detail, &element)["style"]["hyperlink"], json!("https://example.com/report"));

    edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setHyperlink", "slideId": slide, "elementId": element, "url": null }] }),
    )
    .expect("hyperlink clears");
    let detail = slide_detail(&deck, &slide).expect("slide detail");
    assert_eq!(element_entry(&detail, &element)["style"]["hyperlink"], Value::Null);

    for operation in [
        json!({ "op": "element.setHyperlink", "slideId": slide, "elementId": element }),
        json!({ "op": "element.setHyperlink", "slideId": slide, "elementId": element, "url": "   " }),
        json!({ "op": "element.setHyperlink", "slideId": slide, "elementId": "element-missing", "url": "https://x.test" }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("invalid hyperlink");
        assert!(error.code == "session/argument-invalid" || error.code == "object/not-found");
    }
    assert_eq!(deck.revision(), revision + 2, "failed hyperlinks consume no revision");

    let (mut shapes, shape_slide, shape, _) = two_shapes();
    let error = edit(
        &mut shapes,
        &json!({ "operations": [{ "op": "element.setHyperlink", "slideId": shape_slide, "elementId": shape, "url": "https://x.test" }] }),
    )
    .expect_err("a shape has no text runs");
    assert_eq!(error.code, "session/argument-invalid");
}

#[test]
fn deck_title_sets_trims_and_clears() {
    let (mut deck, _slide) = background_deck();
    let revision = deck.revision();

    let report = edit(&mut deck, &json!({ "operations": [{ "op": "deck.setTitle", "title": "  Q3 Review  " }] }))
        .expect("title commits");
    assert_eq!(report["results"][0], Value::Null);
    assert_eq!(report["revision"], revision + 1);
    assert_eq!(summary(&deck)["title"], json!("Q3 Review"), "the title is trimmed");

    edit(&mut deck, &json!({ "operations": [{ "op": "deck.setTitle", "title": null }] })).expect("title clears");
    assert_eq!(summary(&deck)["title"], Value::Null);

    for operation in [
        json!({ "op": "deck.setTitle", "title": "   " }),
        json!({ "op": "deck.setTitle", "title": 3 }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("bad title");
        assert_eq!(error.code, "session/argument-invalid");
    }
    assert_eq!(deck.revision(), revision + 2, "failed titles consume no revision");
}

#[test]
fn slide_add_can_seed_a_title_and_notes() {
    let (mut deck, _slide) = background_deck();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "slide.add", "title": "  Agenda  ", "notes": "  Talk track  " }] }),
    )
    .expect("add a titled slide");
    assert_eq!(report["revision"], revision + 1);
    let slides = summary(&deck)["slides"].as_array().expect("slides").clone();
    let added = slides.last().expect("the new slide");
    assert_eq!(added["title"], json!("Agenda"), "the title is trimmed");
    assert_eq!(added["notes"], json!("Talk track"), "the notes are trimmed");

    for operation in [
        json!({ "op": "slide.add", "title": "   " }),
        json!({ "op": "slide.add", "notes": "" }),
        json!({ "op": "slide.add", "title": 5 }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("bad slide seed");
        assert_eq!(error.code, "session/argument-invalid");
    }
    assert_eq!(deck.revision(), revision + 1, "failed slides consume no revision");
}

#[test]
fn deck_size_accepts_named_ratios_and_custom_points() {
    let (mut deck, _slide) = background_deck();
    let revision = deck.revision();

    let report = edit(&mut deck, &json!({ "operations": [{ "op": "deck.setSize", "ratio": "4:3" }] }))
        .expect("named size commits");
    assert_eq!(report["results"][0], Value::Null);
    assert_eq!(report["revision"], revision + 1);
    let sized = summary(&deck);
    assert_eq!(sized["size"]["widthPt"], json!(720.0), "4:3 is 10 inches wide");
    assert_eq!(sized["size"]["heightPt"], json!(540.0), "4:3 is 7.5 inches tall");

    edit(&mut deck, &json!({ "operations": [{ "op": "deck.setSize", "width": 960.0, "height": 540.0 }] }))
        .expect("custom size commits");
    let sized = summary(&deck);
    assert_eq!(sized["size"]["widthPt"], json!(960.0));
    assert_eq!(sized["size"]["heightPt"], json!(540.0));

    for operation in [
        json!({ "op": "deck.setSize", "ratio": "5:4" }),
        json!({ "op": "deck.setSize", "width": 960.0 }),
        json!({ "op": "deck.setSize", "width": 0.0, "height": 540.0 }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("bad size");
        assert_eq!(error.code, "session/argument-invalid");
    }
    assert_eq!(deck.revision(), revision + 2, "failed sizes consume no revision");
}

#[test]
fn a_failing_duplicate_rolls_back_the_whole_batch() {
    let (mut deck, slide, sources) = duplicate_deck();
    let revision = deck.revision();
    let error = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "element.duplicate", "slideId": slide, "elementId": sources[0], "offsetXPt": 10.0 },
                { "op": "element.duplicate", "slideId": slide, "elementId": "element-missing" },
            ],
        }),
    )
    .expect_err("the second duplicate fails");
    assert_eq!(error.code, "object/not-found");
    assert_eq!(deck.revision(), revision, "no revision is consumed");
    assert_eq!(element_ids(&deck, &slide), sources, "the first duplicate is rolled back");
}

fn replace_deck() -> (Deck, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let report = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "slide.add", "as": "$tx:first" },
                { "op": "text.add", "slideId": "$tx:first", "text": "Revenue for Q4 beats plan; revenues rose" },
                { "op": "slide.add", "as": "$tx:second" },
                {
                    "op": "text.add",
                    "slideId": "$tx:second",
                    "text": { "paragraphs": [{ "runs": [{ "text": "revenue" }, { "text": " is up" }], "level": 0 }] },
                },
                {
                    "op": "table.add",
                    "slideId": "$tx:second",
                    "columnWidthsPt": [100.0, 100.0],
                    "rowHeightsPt": [30.0, 30.0],
                    "rows": [
                        [{ "text": "Revenue" }, { "text": "Q4 revenue plan" }],
                        [{ "text": "Cost" }, { "text": "flat" }],
                    ],
                },
            ],
        }),
    )
    .expect("replace deck builds");
    let first = report["resolved"]["$tx:first"].as_str().expect("first slide id").to_string();
    let second = report["resolved"]["$tx:second"].as_str().expect("second slide id").to_string();
    (deck, first, second)
}

#[test]
fn deck_replace_text_rewrites_text_elements_and_table_cells() {
    let (mut deck, first, second) = replace_deck();
    let revision = deck.revision();
    let report =
        edit(&mut deck, &json!({ "operations": [{ "op": "deck.replaceText", "find": "revenue", "replace": "income" }] }))
            .expect("replacement applies");
    assert_eq!(report["results"][0], "5", "every run-level match is replaced");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");

    let detail = slide_detail(&deck, &first).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "income for Q4 beats plan; incomes rose");

    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "income is up");
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["text"], "income");
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][1]["text"], "Q4 income plan");
    assert_eq!(detail["elements"][1]["content"]["rows"][1]["cells"][0]["text"], "Cost");
}

#[test]
fn deck_replace_text_honours_case_and_whole_word_options() {
    let (mut deck, first, second) = replace_deck();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "deck.replaceText", "find": "revenue", "replace": "income", "caseSensitive": true, "wholeWord": true }],
        }),
    )
    .expect("case-sensitive whole-word replacement applies");
    assert_eq!(report["results"][0], "2", "only exact-case whole words are replaced");
    let detail = slide_detail(&deck, &first).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "Revenue for Q4 beats plan; revenues rose");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "income is up");
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["text"], "Revenue");
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][1]["text"], "Q4 income plan");

    let (mut deck, first, second) = replace_deck();
    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "deck.replaceText", "find": "revenue", "replace": "income", "wholeWord": true }] }),
    )
    .expect("whole-word replacement applies");
    assert_eq!(report["results"][0], "4", "the plural form is left alone");
    let detail = slide_detail(&deck, &first).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "income for Q4 beats plan; revenues rose");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "income is up");
}

#[test]
fn deck_replace_text_restricts_rewrites_to_listed_slides() {
    let (mut deck, first, second) = replace_deck();
    let report = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "deck.replaceText", "find": "revenue", "replace": "income", "slideIds": ["slide-missing", second] }],
        }),
    )
    .expect("filtered replacement applies");
    assert_eq!(report["results"][0], "3", "only the listed slide is rewritten and unknown ids are ignored");
    let detail = slide_detail(&deck, &first).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "Revenue for Q4 beats plan; revenues rose");
}

#[test]
fn deck_replace_text_leaves_matches_spanning_runs_untouched() {
    let (mut deck, _first, second) = replace_deck();
    let report =
        edit(&mut deck, &json!({ "operations": [{ "op": "deck.replaceText", "find": "revenue is", "replace": "income" }] }))
            .expect("cross-run replacement applies");
    assert_eq!(report["results"][0], "0", "a match spanning two runs is not replaced");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][0]["text"], "revenue is up");
}

#[test]
fn deck_replace_text_rejects_bad_arguments() {
    let (mut deck, _first, _second) = replace_deck();
    let revision = deck.revision();

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "deck.replaceText", "find": "", "replace": "income" }] }))
        .expect_err("an empty needle is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "deck.replaceText", "find": "revenue" }] }))
        .expect_err("a missing replacement is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "deck.replaceText", "find": "revenue", "replace": "income", "wholeWord": "yes" }],
        }),
    )
    .expect_err("a non-boolean option is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed replacements consume no revision");
}

fn asset_deck() -> (Deck, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let report = edit(
        &mut deck,
        &json!({
            "operations": [
                { "op": "slide.add", "as": "$tx:first" },
                { "op": "asset.add", "as": "$tx:used", "name": "logo.png", "mediaType": "image/png", "base64": "aGVsbG8=" },
                { "op": "asset.add", "as": "$tx:free", "name": "spare.png", "mediaType": "image/png", "base64": "d29ybGQ=" },
                { "op": "image.add", "slideId": "$tx:first", "assetId": "$tx:used", "alt": "Logo" },
            ],
        }),
    )
    .expect("asset deck builds");
    let used = report["resolved"]["$tx:used"].as_str().expect("used asset id").to_string();
    let free = report["resolved"]["$tx:free"].as_str().expect("free asset id").to_string();
    (deck, used, free)
}

fn asset_ids(deck: &Deck) -> Vec<String> {
    deck.document().assets.iter().map(|asset| asset.id.as_str().to_string()).collect()
}

#[test]
fn asset_remove_drops_an_unused_asset() {
    let (mut deck, used, free) = asset_deck();
    let revision = deck.revision();
    let report = edit(&mut deck, &json!({ "operations": [{ "op": "asset.remove", "assetId": free }] }))
        .expect("removing an unused asset applies");
    assert!(report["results"][0].is_null(), "removal creates no object");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");
    let ids = asset_ids(&deck);
    assert_eq!(ids.len(), 1, "the spare asset is gone");
    assert_eq!(ids[0], used, "the referenced asset stays");
}

#[test]
fn asset_remove_refuses_referenced_and_unknown_assets() {
    let (mut deck, used, _free) = asset_deck();
    let revision = deck.revision();

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "asset.remove", "assetId": used }] }))
        .expect_err("a referenced asset is refused");
    assert_eq!(error.code, "object/unsupported");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "asset.remove", "assetId": "asset-missing" }] }))
        .expect_err("an unknown asset is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "asset.remove" }] })).expect_err("a missing id is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed removals consume no revision");
    assert_eq!(asset_ids(&deck).len(), 2, "both assets survive the failed removals");
}

#[test]
fn slide_set_layout_applies_and_clears_the_layout() {
    let (mut deck, slide) = background_deck();
    let layout = deck.document().layouts.first().expect("the default deck has a layout").id.as_str().to_string();
    let revision = deck.revision();
    let report = edit(&mut deck, &json!({ "operations": [{ "op": "slide.setLayout", "slideId": slide, "layout": layout }] }))
        .expect("applying a layout commits");
    assert!(report["results"][0].is_null(), "no object is created");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");
    assert_eq!(slide_detail(&deck, &slide).expect("slide exists")["layout"], layout);

    edit(&mut deck, &json!({ "operations": [{ "op": "slide.setLayout", "slideId": slide, "layout": null }] }))
        .expect("clearing the layout commits");
    assert!(slide_detail(&deck, &slide).expect("slide exists")["layout"].is_null());
}

#[test]
fn slide_set_layout_rejects_unknown_objects_and_arguments() {
    let (mut deck, slide) = background_deck();
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "slide.setLayout", "slideId": "slide-missing", "layout": "layout-1" }] }),
    )
    .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    let error =
        edit(&mut deck, &json!({ "operations": [{ "op": "slide.setLayout", "slideId": slide, "layout": "layout-missing" }] }))
            .expect_err("an unknown layout is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.setLayout", "slideId": slide }] }))
        .expect_err("a missing layout is refused");
    assert_eq!(error.code, "session/argument-invalid");

    let error = edit(&mut deck, &json!({ "operations": [{ "op": "slide.setLayout", "slideId": slide, "layout": 42 }] }))
        .expect_err("a non-string layout is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed layout edits consume no revision");
    assert!(slide_detail(&deck, &slide).expect("slide exists")["layout"].is_null(), "the slide keeps its layout");
}

#[test]
fn element_set_line_strokes_and_clears_a_shape() {
    let (mut deck, slide, first, second) = two_shapes();
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setLine", "slideId": slide, "elementId": first, "line": { "color": "#0000FF", "widthPt": 2.5 } }] }),
    )
    .expect("stroking a shape commits");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    let stroke = detail["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .find(|element| element["id"] == json!(first))
        .expect("the stroked element")["content"]["line"]
        .clone();
    assert_eq!(stroke["color"], json!("#0000FF"));
    assert_eq!(stroke["widthPt"], json!(2.5));
    let untouched = detail["elements"]
        .as_array()
        .expect("elements")
        .iter()
        .find(|element| element["id"] == json!(second))
        .expect("the other element");
    assert!(untouched["content"]["line"].is_null(), "the other shape keeps its line");

    edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setLine", "slideId": slide, "elementId": first, "line": null }] }),
    )
    .expect("clearing the line commits");
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert!(
        detail["elements"]
            .as_array()
            .expect("elements")
            .iter()
            .find(|element| element["id"] == json!(first))
            .expect("the stroked element")["content"]["line"]
            .is_null(),
        "the cleared shape keeps no line"
    );
}

#[test]
fn element_set_line_refuses_non_shapes_and_bad_arguments() {
    let (mut deck, _first, second) = replace_deck();
    let text_element =
        slide_detail(&deck, &second).expect("slide exists")["elements"][0]["id"].as_str().expect("element id").to_string();
    let revision = deck.revision();
    for operation in [
        json!({ "op": "element.setLine", "slideId": second, "elementId": text_element, "line": { "color": "#0000FF" } }),
        json!({ "op": "element.setLine", "slideId": second, "elementId": "element-missing", "line": { "color": "#0000FF" } }),
        json!({ "op": "element.setLine", "slideId": "slide-missing", "elementId": text_element, "line": { "color": "#0000FF" } }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("the target is refused");
        assert!(
            error.code == "object/type-mismatch" || error.code == "object/not-found",
            "unexpected code {}",
            error.code
        );
    }
    for line in [json!({}), json!({ "widthPt": 0 }), json!({ "widthPt": "wide" }), json!({ "color": 42 })] {
        let error = edit(
            &mut deck,
            &json!({ "operations": [{ "op": "element.setLine", "slideId": second, "elementId": text_element, "line": line }] }),
        )
        .expect_err("the line is refused");
        assert_eq!(error.code, "session/argument-invalid");
    }
    let error = edit(&mut deck, &json!({ "operations": [{ "op": "element.setLine", "slideId": second, "elementId": text_element }] }))
        .expect_err("a missing line is refused");
    assert_eq!(error.code, "session/argument-invalid");
    assert_eq!(deck.revision(), revision, "failed lines consume no revision");
}

#[test]
fn table_set_cell_rewrites_text_and_fill_and_composes() {
    let (mut deck, _first, second) = replace_deck();
    let element =
        slide_detail(&deck, &second).expect("slide exists")["elements"][1]["id"].as_str().expect("table id").to_string();
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({ "operations": [
            { "op": "table.setCell", "slideId": second, "elementId": element, "row": 0, "column": 1, "text": "Q4 actual", "fill": "#FFFF00" },
            { "op": "table.setCell", "slideId": second, "elementId": element, "row": 1, "column": 0, "text": "Margin" },
        ] }),
    )
    .expect("cell edits commit");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");

    let detail = slide_detail(&deck, &second).expect("slide exists");
    let cells = &detail["elements"][1]["content"]["rows"];
    assert_eq!(cells[0]["cells"][1]["text"], json!("Q4 actual"), "the first cell's text changed");
    assert_eq!(cells[0]["cells"][1]["fill"], json!("#FFFF00"), "the first cell's fill changed");
    assert_eq!(cells[1]["cells"][0]["text"], json!("Margin"), "the queued second edit composes with the first");
    assert_eq!(cells[1]["cells"][1]["text"], json!("flat"), "untouched cells stay");

    edit(
        &mut deck,
        &json!({ "operations": [{ "op": "table.setCell", "slideId": second, "elementId": element, "row": 0, "column": 1, "fill": null }] }),
    )
    .expect("clearing the fill commits");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert!(
        detail["elements"][1]["content"]["rows"][0]["cells"][1]["fill"].is_null(),
        "the cleared cell keeps no fill"
    );
}

#[test]
fn table_geometry_rewrites_column_widths_and_row_heights() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let element = detail["elements"][1]["id"].as_str().expect("table id").to_string();
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0]));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["heightPt"], json!(30.0));

    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({ "operations": [
            { "op": "table.setColumnWidths", "slideId": second, "elementId": element, "columnWidthsPt": [120.0, 80.0] },
            { "op": "table.setRowHeights", "slideId": second, "elementId": element, "rowHeightsPt": [42.5, 25.0] },
        ] }),
    )
    .expect("table geometry commits");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([120.0, 80.0]));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["heightPt"], json!(42.5));
    assert_eq!(detail["elements"][1]["content"]["rows"][1]["heightPt"], json!(25.0));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["text"], json!("Revenue"), "cells stay put");

    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_string();
    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "table.setColumnWidths", "slideId": second, "elementId": text_element, "columnWidthsPt": [100.0] }] }),
    )
    .expect_err("text is not a table");
    assert_eq!(error.code, "object/type-mismatch");
    for operation in [
        json!({ "op": "table.setColumnWidths", "slideId": second, "elementId": element, "columnWidthsPt": [100.0] }),
        json!({ "op": "table.setRowHeights", "slideId": second, "elementId": element, "rowHeightsPt": [30.0] }),
        json!({ "op": "table.setColumnWidths", "slideId": second, "elementId": element, "columnWidthsPt": [0.0, 100.0] }),
        json!({ "op": "table.setRowHeights", "slideId": second, "elementId": element, "rowHeightsPt": [] }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("bad geometry is refused");
        assert_eq!(error.code, "session/argument-invalid");
    }
}

#[test]
fn table_set_cell_refuses_non_tables_and_bad_coordinates() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let text_element = detail["elements"][0]["id"].as_str().expect("element id").to_string();
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_string();
    let revision = deck.revision();
    let error =
        edit(&mut deck, &json!({ "operations": [{ "op": "table.setCell", "slideId": second, "elementId": text_element, "row": 0, "column": 0, "text": "x" }] }))
            .expect_err("text is not a table");
    assert_eq!(error.code, "object/type-mismatch");
    for operation in [
        json!({ "op": "table.setCell", "slideId": second, "elementId": table, "row": 2, "column": 0, "text": "x" }),
        json!({ "op": "table.setCell", "slideId": second, "elementId": table, "row": 0, "column": 2, "text": "x" }),
        json!({ "op": "table.setCell", "slideId": second, "elementId": table, "row": 0, "column": 0 }),
        json!({ "op": "table.setCell", "slideId": second, "elementId": table, "row": 0, "column": 0, "fill": 7 }),
        json!({ "op": "table.setCell", "slideId": second, "elementId": table, "row": -1, "column": 0, "text": "x" }),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("the cell request is refused");
        assert_eq!(error.code, "session/argument-invalid");
    }
    assert_eq!(deck.revision(), revision, "failed cell edits consume no revision");
}

#[test]
fn element_set_fill_repaints_and_clears_a_shape() {
    let (mut deck, slide, first, _second) = two_shapes();
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setFill", "slideId": slide, "elementId": first, "fill": "#0000FF" }] }),
    )
    .expect("repainting a shape commits");
    assert!(report["results"][0].is_null(), "no object is created");
    assert_eq!(report["revision"], revision + 1, "one script bumps one revision");
    let page = svg(&deck);
    assert!(page.contains("fill=\"#0000FF\"") && !page.contains("fill=\"#FF0000\""), "the restyled shape paints blue: {page}");
    assert!(page.contains("fill=\"#00FF00\""), "the untouched shape keeps its paint");

    edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setFill", "slideId": slide, "elementId": first, "fill": null }] }),
    )
    .expect("clearing the fill commits");
    let page = svg(&deck);
    assert!(!page.contains("fill=\"#0000FF\""), "the cleared shape no longer paints blue: {page}");
    assert!(page.contains("fill=\"none\""), "the cleared shape renders unpainted: {page}");
}

#[test]
fn element_set_fill_refuses_non_shapes_and_bad_arguments() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let text_element = detail["elements"][0]["id"].as_str().expect("element id").to_string();
    let revision = deck.revision();

    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setFill", "slideId": second, "elementId": text_element, "fill": "#0000FF" }] }),
    )
    .expect_err("text carries no fill");
    assert_eq!(error.code, "object/type-mismatch");

    let error = edit(
        &mut deck,
        &json!({
            "operations": [{ "op": "element.setFill", "slideId": second, "elementId": "element-missing", "fill": "#0000FF" }],
        }),
    )
    .expect_err("an unknown element is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setFill", "slideId": "slide-missing", "elementId": text_element, "fill": "#0000FF" }] }),
    )
    .expect_err("an unknown slide is refused");
    assert_eq!(error.code, "object/not-found");

    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "element.setFill", "slideId": second, "elementId": text_element, "fill": 42 }] }),
    )
    .expect_err("a non-string fill is refused");
    assert_eq!(error.code, "session/argument-invalid");

    assert_eq!(deck.revision(), revision, "failed fills consume no revision");
}

#[test]
fn table_cell_style_stamps_every_run_of_one_cell() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_owned();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.setCellStyle",
            "slideId": second,
            "elementId": table,
            "row": 0,
            "column": 1,
            "bold": true,
            "italic": true,
            "sizePt": 18,
            "color": "#112233",
            "align": "center",
        }] }),
    )
    .expect("cell style applies");
    assert_eq!(report["revision"], revision + 1);
    assert_eq!(report["results"][0], Value::Null, "style edits have no payload");

    let detail = slide_detail(&deck, &second).expect("slide exists");
    let cell = &detail["elements"][1]["content"]["rows"][0]["cells"][1];
    assert_eq!(cell["text"], json!("Q4 revenue plan"));
    assert_eq!(cell["style"]["bold"], json!(true));
    assert_eq!(cell["style"]["italic"], json!(true));
    assert_eq!(cell["style"]["sizePt"], json!(18.0));
    assert_eq!(cell["style"]["color"], json!("#112233"));
    assert_eq!(cell["style"]["align"], json!("center"));
    assert_eq!(
        detail["elements"][1]["content"]["rows"][0]["cells"][0]["style"]["bold"],
        Value::Null,
        "the neighbouring cell keeps its own styling"
    );
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["style"]["align"], Value::Null);

    let before = deck.revision();
    for operation in [
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": table, "row": 9, "column": 0, "bold": true }),
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": table, "row": 0, "column": 9, "bold": true }),
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": table, "row": 0, "column": 0 }),
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": table, "row": 0, "column": 0, "align": "middle" }),
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": "shape-999", "row": 0, "column": 0, "bold": true }),
        json!({ "op": "table.setCellStyle", "slideId": second, "elementId": text_element, "row": 0, "column": 0, "bold": true }),
    ] {
        edit(&mut deck, &json!({ "operations": [operation] })).expect_err("invalid cell style");
    }
    assert_eq!(deck.revision(), before, "failed requests leave the revision untouched");
}


#[test]
fn table_rows_insert_and_delete_with_heights_and_guards() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.insertRow",
            "slideId": second,
            "elementId": table,
            "row": 2,
            "cells": ["Tax", "10"],
        }] }),
    )
    .expect("append a row");
    assert_eq!(report["revision"], revision + 1);
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[2]["cells"][0]["text"], json!("Tax"));
    assert_eq!(rows[2]["cells"][1]["text"], json!("10"));
    assert_eq!(rows[2]["heightPt"], json!(30.0), "the new row inherits the last row height");

    edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.insertRow",
            "slideId": second,
            "elementId": table,
            "row": 0,
            "heightPt": 42.5,
        }] }),
    )
    .expect("insert a blank row first");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["cells"][0]["text"], json!(""));
    assert_eq!(rows[0]["cells"][1]["text"], json!(""));
    assert_eq!(rows[0]["heightPt"], json!(42.5));

    edit(&mut deck, &json!({ "operations": [{ "op": "table.deleteRow", "slideId": second, "elementId": table, "row": 0 }] }))
        .expect("delete the blank row");
    edit(&mut deck, &json!({ "operations": [{ "op": "table.deleteRow", "slideId": second, "elementId": table, "row": 2 }] }))
        .expect("delete the appended row");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["cells"][0]["text"], json!("Revenue"));
    assert_eq!(rows[1]["cells"][0]["text"], json!("Cost"));

    edit(&mut deck, &json!({ "operations": [{ "op": "table.moveRow", "slideId": second, "elementId": table, "row": 1, "index": 0 }] }))
        .expect("move the last row first");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["cells"][0]["text"], json!("Cost"));
    assert_eq!(rows[1]["cells"][0]["text"], json!("Revenue"));
    edit(&mut deck, &json!({ "operations": [{ "op": "table.moveRow", "slideId": second, "elementId": table, "row": 0, "index": 1 }] }))
        .expect("move it back");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["cells"][0]["text"], json!("Revenue"));
    assert_eq!(rows[1]["cells"][0]["text"], json!("Cost"));

    let (mut errors, _first, second) = replace_deck();
    let detail = slide_detail(&errors, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_owned();
    edit(&mut errors, &json!({ "operations": [{ "op": "table.deleteRow", "slideId": second, "elementId": table, "row": 1 }] }))
        .expect("shrink to one row");
    let before = errors.revision();
    for operation in [
        json!({ "op": "table.insertRow", "slideId": second, "elementId": table, "row": 9, "cells": ["x"] }),
        json!({ "op": "table.insertRow", "slideId": second, "elementId": table, "row": 0, "cells": ["x"] }),
        json!({ "op": "table.insertRow", "slideId": second, "elementId": table, "row": 0, "heightPt": 0 }),
        json!({ "op": "table.insertRow", "slideId": second, "elementId": text_element, "row": 0 }),
        json!({ "op": "table.deleteRow", "slideId": second, "elementId": table, "row": 9 }),
        json!({ "op": "table.deleteRow", "slideId": second, "elementId": table, "row": 0 }),
        json!({ "op": "table.deleteRow", "slideId": second, "elementId": text_element, "row": 0 }),
        json!({ "op": "table.moveRow", "slideId": second, "elementId": table, "row": 9, "index": 0 }),
        json!({ "op": "table.moveRow", "slideId": second, "elementId": table, "row": 0, "index": 9 }),
        json!({ "op": "table.moveRow", "slideId": second, "elementId": text_element, "row": 0, "index": 0 }),
    ] {
        edit(&mut errors, &json!({ "operations": [operation] })).expect_err("invalid row edit");
    }
    assert_eq!(errors.revision(), before, "failed requests leave the revision untouched");
}

#[test]
fn table_move_column_moves_cells_and_widths() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_owned();

    edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.insertColumn",
            "slideId": second,
            "elementId": table,
            "column": 2,
            "cells": ["Note", "More"],
            "widthPt": 50,
        }] }),
    )
    .expect("append a third column");

    edit(&mut deck, &json!({ "operations": [{ "op": "table.moveColumn", "slideId": second, "elementId": table, "column": 0, "index": 2 }] }))
        .expect("move the first column last");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 50.0, 100.0]));
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["cells"][0]["text"], json!("Q4 revenue plan"));
    assert_eq!(rows[0]["cells"][1]["text"], json!("Note"));
    assert_eq!(rows[0]["cells"][2]["text"], json!("Revenue"));
    assert_eq!(rows[1]["cells"][0]["text"], json!("flat"));
    assert_eq!(rows[1]["cells"][1]["text"], json!("More"));
    assert_eq!(rows[1]["cells"][2]["text"], json!("Cost"));

    edit(&mut deck, &json!({ "operations": [{ "op": "table.moveColumn", "slideId": second, "elementId": table, "column": 2, "index": 1 }] }))
        .expect("move the last column to the middle");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0, 50.0]));
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["cells"][0]["text"], json!("Q4 revenue plan"));
    assert_eq!(rows[0]["cells"][1]["text"], json!("Revenue"));
    assert_eq!(rows[0]["cells"][2]["text"], json!("Note"));
    assert_eq!(rows[1]["cells"][0]["text"], json!("flat"));
    assert_eq!(rows[1]["cells"][1]["text"], json!("Cost"));
    assert_eq!(rows[1]["cells"][2]["text"], json!("More"));

    let before = slide_detail(&deck, &second).expect("slide exists");
    let revision = deck.revision();
    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "table.moveColumn", "slideId": second, "elementId": table, "column": 1, "index": 1 }] }),
    )
    .expect("a same-index move is a no-op");
    assert_eq!(report["revision"], revision + 1, "a no-op still commits its batch");
    assert_eq!(slide_detail(&deck, &second).expect("slide exists"), before, "the table is unchanged");

    let revision = deck.revision();
    for operation in [
        json!({ "op": "table.moveColumn", "slideId": second, "elementId": table, "column": 9, "index": 0 }),
        json!({ "op": "table.moveColumn", "slideId": second, "elementId": table, "column": 0, "index": 9 }),
        json!({ "op": "table.moveColumn", "slideId": second, "elementId": text_element, "column": 0, "index": 0 }),
    ] {
        edit(&mut deck, &json!({ "operations": [operation] })).expect_err("invalid column move");
    }
    assert_eq!(deck.revision(), revision, "failed requests leave the revision untouched");
}

/// Builds one table whose first row starts with a cell spanning two columns.
fn horizontally_merged_deck() -> (Deck, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut table = String::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            table = edit
                .add_table(
                    &slide,
                    TableElementInput {
                        column_widths_emu: vec![1_270_000, 1_270_000, 1_270_000],
                        rows: vec![
                            TableRow {
                                height_emu: 508_000,
                                cells: vec![TableCell::plain("A"), TableCell::plain("B"), TableCell::plain("C")],
                            },
                            TableRow {
                                height_emu: 508_000,
                                cells: vec![TableCell::plain("1"), TableCell::plain("2"), TableCell::plain("3")],
                            },
                        ],
                        transform: Some(Transform::from_points(30.0, 160.0, 300.0, 60.0).unwrap()),
                        alt: Some("merged grid".to_string()),
                    },
                )?
                .as_str()
                .to_string();
            Ok(slide)
        })
        .expect("merged fixture builds");

    let mut document = deck.document().clone();
    let ElementContent::Table(table_content) = &mut document.slides[0].elements[0].content
    else {
        panic!("the fixture element is a table");
    };
    table_content.rows[0].cells.remove(1);
    table_content.rows[0].cells[0].column_span = 2;
    let deck = Deck::from_document(document).expect("the merged table validates");
    (deck, slide.as_str().to_string(), table)
}

/// Builds one table whose first column starts with a cell spanning two rows.
fn vertically_merged_deck() -> (Deck, String, String) {
    let mut deck = Deck::create(DeckCreateOptions::default());
    let mut table = String::new();
    let slide = deck
        .transaction(None, |edit| {
            let slide = edit.add_slide(SlideAddOptions { layout: None, index: None })?;
            table = edit
                .add_table(
                    &slide,
                    TableElementInput {
                        column_widths_emu: vec![1_270_000, 1_270_000],
                        rows: vec![
                            TableRow { height_emu: 508_000, cells: vec![TableCell::plain("A"), TableCell::plain("B")] },
                            TableRow { height_emu: 508_000, cells: vec![TableCell::plain("1"), TableCell::plain("2")] },
                        ],
                        transform: Some(Transform::from_points(30.0, 160.0, 200.0, 60.0).unwrap()),
                        alt: Some("merged grid".to_string()),
                    },
                )?
                .as_str()
                .to_string();
            Ok(slide)
        })
        .expect("merged fixture builds");

    let mut document = deck.document().clone();
    let ElementContent::Table(table_content) = &mut document.slides[0].elements[0].content
    else {
        panic!("the fixture element is a table");
    };
    table_content.rows[1].cells.remove(0);
    table_content.rows[0].cells[0].row_span = 2;
    let deck = Deck::from_document(document).expect("the merged table validates");
    (deck, slide.as_str().to_string(), table)
}

#[test]
fn table_move_column_refuses_tables_with_horizontal_merges() {
    let (mut deck, slide, table) = horizontally_merged_deck();
    let detail = slide_detail(&deck, &slide).expect("slide exists");
    assert_eq!(
        detail["elements"][0]["content"]["rows"][0]["cells"].as_array().expect("cells").len(),
        2,
        "the fixture merges the first two cells of row 0"
    );

    let revision = deck.revision();
    let error = edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.moveColumn",
            "slideId": slide,
            "elementId": table,
            "column": 0,
            "index": 1,
        }] }),
    )
    .expect_err("a horizontal merge blocks column moves");
    assert_eq!(error.code, "session/argument-invalid");
    assert!(error.message.contains("horizontal merge"), "{}", error.message);
    assert_eq!(deck.revision(), revision, "a refused move leaves the revision untouched");
}

#[test]
fn table_columns_insert_and_delete_with_widths_and_guards() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();

    edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.insertColumn",
            "slideId": second,
            "elementId": table,
            "column": 2,
            "cells": ["Note", "More"],
            "widthPt": 50,
        }] }),
    )
    .expect("append a column");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0, 50.0]));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][2]["text"], json!("Note"));
    assert_eq!(detail["elements"][1]["content"]["rows"][1]["cells"][2]["text"], json!("More"));

    edit(
        &mut deck,
        &json!({ "operations": [{
            "op": "table.insertColumn",
            "slideId": second,
            "elementId": table,
            "column": 0,
        }] }),
    )
    .expect("insert a blank column first");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0, 100.0, 50.0]));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["text"], json!(""));

    edit(&mut deck, &json!({ "operations": [{ "op": "table.deleteColumn", "slideId": second, "elementId": table, "column": 0 }] }))
        .expect("delete the blank column");
    edit(&mut deck, &json!({ "operations": [{ "op": "table.deleteColumn", "slideId": second, "elementId": table, "column": 2 }] }))
        .expect("delete the appended column");
    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0]));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][0]["text"], json!("Revenue"));
    assert_eq!(detail["elements"][1]["content"]["rows"][0]["cells"][1]["text"], json!("Q4 revenue plan"));

    let (mut errors, _first, second) = replace_deck();
    let detail = slide_detail(&errors, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_owned();
    for operation in [
        json!({ "op": "table.insertColumn", "slideId": second, "elementId": table, "column": 9 }),
        json!({ "op": "table.insertColumn", "slideId": second, "elementId": table, "column": 0, "cells": ["x"] }),
        json!({ "op": "table.insertColumn", "slideId": second, "elementId": table, "column": 0, "widthPt": 0 }),
        json!({ "op": "table.insertColumn", "slideId": second, "elementId": text_element, "column": 0 }),
        json!({ "op": "table.deleteColumn", "slideId": second, "elementId": table, "column": 9 }),
        json!({ "op": "table.deleteColumn", "slideId": second, "elementId": text_element, "column": 0 }),
    ] {
        edit(&mut errors, &json!({ "operations": [operation] })).expect_err("invalid column edit");
    }
    edit(&mut errors, &json!({ "operations": [{ "op": "table.deleteColumn", "slideId": second, "elementId": table, "column": 1 }] }))
        .expect("shrink to one column");
    let before = errors.revision();
    edit(&mut errors, &json!({ "operations": [{ "op": "table.deleteColumn", "slideId": second, "elementId": table, "column": 0 }] }))
        .expect_err("a table keeps one column");
    assert_eq!(errors.revision(), before, "failed requests leave the revision untouched");
}

#[test]
fn table_duplicate_row_copies_cells_styles_and_height() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({ "operations": [
            { "op": "table.setCellStyle", "slideId": second, "elementId": table, "row": 0, "column": 0, "bold": true },
            { "op": "table.duplicateRow", "slideId": second, "elementId": table, "row": 0 },
        ] }),
    )
    .expect("duplicate the styled header row");
    assert_eq!(report["revision"], revision + 1, "one script is one revision bump");
    assert_eq!(report["results"][1], json!("1"), "the copy reports its new index");

    let detail = slide_detail(&deck, &second).expect("slide exists");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[1]["cells"][0]["text"], json!("Revenue"));
    assert_eq!(rows[1]["cells"][1]["text"], json!("Q4 revenue plan"));
    assert_eq!(rows[1]["cells"][0]["style"]["bold"], json!(true), "the copy follows the row's current style");
    assert_eq!(rows[1]["heightPt"], json!(30.0), "the copy keeps the row height");
    assert_eq!(rows[2]["cells"][0]["text"], json!("Cost"), "the untouched row shifts down");
}

#[test]
fn table_duplicate_column_copies_cells_and_width() {
    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let revision = deck.revision();

    let report = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "table.duplicateColumn", "slideId": second, "elementId": table, "column": 0 }] }),
    )
    .expect("duplicate the first column");
    assert_eq!(report["revision"], revision + 1);
    assert_eq!(report["results"][0], json!("1"), "the copy reports its new index");

    let detail = slide_detail(&deck, &second).expect("slide exists");
    assert_eq!(detail["elements"][1]["content"]["columnWidthsPt"], json!([100.0, 100.0, 100.0]), "the width is copied too");
    let rows = detail["elements"][1]["content"]["rows"].as_array().expect("rows");
    assert_eq!(rows[0]["cells"].as_array().expect("cells").len(), 3);
    assert_eq!(rows[0]["cells"][0]["text"], json!("Revenue"));
    assert_eq!(rows[0]["cells"][1]["text"], json!("Revenue"));
    assert_eq!(rows[0]["cells"][2]["text"], json!("Q4 revenue plan"));
    assert_eq!(rows[1]["cells"][1]["text"], json!("Cost"));
    assert_eq!(rows[1]["cells"][2]["text"], json!("flat"));
}

#[test]
fn table_duplicate_refuses_merges_and_bad_arguments() {
    let (mut deck, slide, table) = vertically_merged_deck();
    let revision = deck.revision();
    let error = edit(&mut deck, &json!({ "operations": [{ "op": "table.duplicateRow", "slideId": slide, "elementId": table, "row": 0 }] }))
        .expect_err("a vertical merge blocks row duplication");
    assert_eq!(error.code, "session/argument-invalid");
    assert!(error.message.contains("vertical merge"), "{}", error.message);

    let (mut deck, slide, table) = horizontally_merged_deck();
    let error = edit(
        &mut deck,
        &json!({ "operations": [{ "op": "table.duplicateColumn", "slideId": slide, "elementId": table, "column": 0 }] }),
    )
    .expect_err("a horizontal merge blocks column duplication");
    assert_eq!(error.code, "session/argument-invalid");
    assert!(error.message.contains("horizontal merge"), "{}", error.message);
    assert_eq!(deck.revision(), revision, "a refused duplication leaves the revision untouched");

    let (mut deck, _first, second) = replace_deck();
    let detail = slide_detail(&deck, &second).expect("slide exists");
    let table = detail["elements"][1]["id"].as_str().expect("table id").to_owned();
    let text_element = detail["elements"][0]["id"].as_str().expect("text id").to_owned();
    let revision = deck.revision();
    for (operation, code) in [
        (json!({ "op": "table.duplicateRow", "slideId": second, "elementId": table, "row": 9 }), "session/argument-invalid"),
        (json!({ "op": "table.duplicateRow", "slideId": second, "elementId": text_element, "row": 0 }), "object/type-mismatch"),
        (json!({ "op": "table.duplicateRow", "slideId": "slide-missing", "elementId": table, "row": 0 }), "object/not-found"),
        (json!({ "op": "table.duplicateRow", "slideId": second, "elementId": "element-missing", "row": 0 }), "object/not-found"),
        (json!({ "op": "table.duplicateColumn", "slideId": second, "elementId": table, "column": 9 }), "session/argument-invalid"),
        (json!({ "op": "table.duplicateColumn", "slideId": second, "elementId": text_element, "column": 0 }), "object/type-mismatch"),
        (json!({ "op": "table.duplicateColumn", "slideId": "slide-missing", "elementId": table, "column": 0 }), "object/not-found"),
        (json!({ "op": "table.duplicateColumn", "slideId": second, "elementId": "element-missing", "column": 0 }), "object/not-found"),
    ] {
        let error = edit(&mut deck, &json!({ "operations": [operation] })).expect_err("invalid duplication");
        assert_eq!(error.code, code, "{} reports the right code", operation["op"].as_str().expect("op name"));
    }
    assert_eq!(deck.revision(), revision, "failed requests leave the revision untouched");
}
