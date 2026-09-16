#![cfg(feature = "markdown")]

use std::time::Duration;

use icmd::advanced::{Commit, Lower, Renderer, Runtime, RuntimeError, Stage};
use icmd::events::DispatchOutcome;
use icmd::{Attr, Component, ComponentContext, MarkdownProps, Node, Props, Size, markdown, ui};

fn render(node: Node, viewport: Size) -> String {
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    input.send(node).unwrap();
    output
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime did not produce a frame")
        .expect("renderer failed")
}

fn painted(frame: &str) -> String {
    let mut output = String::new();
    let mut chars = frame.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }
        if chars.next() == Some('[') {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    output
}

fn source(value: &str) -> Node {
    markdown
        .props(MarkdownProps {
            text: Attr::Set(value.to_owned()),
        })
        .node()
}

#[test]
fn public_root_and_macro_surfaces_construct_markdown() {
    let root = source("# Root");
    let macro_node = ui! { <markdown text="**Macro**" /> };
    let _ = (
        root,
        macro_node,
        icmd::widgets::markdown,
        icmd::prelude::markdown,
    );
    let _: MarkdownProps = Default::default();
}

#[test]
fn renders_commonmark_and_gfm_blocks() {
    let list_frame = painted(&render(
        source("- **bold**\n- nested:\n  - child"),
        Size::new(40, 8),
    ));
    assert!(list_frame.contains("bold") && list_frame.contains("child"));
    let node = source(
        "# Heading\n\nA **bold** *italic* ~~strike~~ `code` and [link](https://example.test).\n\n- one\n- two\n- [x] done\n\n3. three\n4. four\n\n> quote\n\n> [!NOTE]\n> alert\n\n```rust\nlet x = 1;\n```\n\n---\n\n| left | right |\n| :--- | ---: |\n| a | b |",
    );
    let frame = painted(&render(node, Size::new(80, 24)));
    for expected in [
        "Heading",
        "A bold italic strike code and link",
        "• one",
        "• two",
        "[x] done",
        "3. three",
        "4. four",
        "quote",
        "NOTE",
        "alert",
        "let x = 1;",
        "left",
        "right",
    ] {
        assert!(
            frame.contains(expected),
            "missing {expected:?} in {frame:?}"
        );
    }
    assert!(!frame.contains("# Heading"));
}

#[test]
fn heading_levels_render_without_markdown_markers() {
    let frame = painted(&render(
        source("# One\n\n## Two\n\n### Three\n\n#### Four\n\n##### Five\n\n###### Six"),
        Size::new(30, 12),
    ));
    for expected in ["One", "Two", "Three", "Four", "Five", "Six"] {
        assert!(
            frame.contains(expected),
            "missing {expected:?} in {frame:?}"
        );
    }
    for marker in [
        "# One",
        "## Two",
        "### Three",
        "#### Four",
        "##### Five",
        "###### Six",
    ] {
        assert!(!frame.contains(marker), "found heading marker {marker:?}");
    }
}

#[test]
fn renders_images_as_selectable_text_and_footnotes() {
    let frame = painted(&render(
        source("![logo](https://example.test/logo.png)\n\nText[^1]\n\n[^1]: footnote"),
        Size::new(80, 12),
    ));
    assert!(frame.contains("[image: logo] (https://example.test/logo.png)"));
    assert!(frame.contains("[1]"));
    assert!(frame.contains("footnote"));
}

#[test]
fn empty_text_is_empty_and_children_are_rejected() {
    let empty = markdown.node();
    let frame = painted(&render(empty, Size::new(20, 3)));
    assert!(!frame.contains("#"));

    let child = markdown.apply(Props::with_parts(
        icmd::DomProps::default(),
        vec![icmd::text("not accepted")],
        MarkdownProps::default(),
    ));
    let (commit, _, _) = Commit::new_with_events(Size::new(20, 3));
    let mut runtime = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(Size::new(20, 3)).unwrap())
        .start_handle();
    runtime.input().send(child).unwrap();
    let error = runtime
        .errors()
        .recv_timeout(Duration::from_secs(1))
        .expect("children must be rejected by the lower stage");
    assert!(matches!(
        error,
        RuntimeError::StagePanicked {
            stage: Stage::Lower
        }
    ));
    runtime.close_input();
    runtime
        .shutdown(icmd::advanced::ShutdownPolicy::default())
        .unwrap();
}

#[test]
fn markdown_region_supports_keyboard_selection() {
    let (commit, _, dispatcher) = Commit::new_with_events(Size::new(40, 6));
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(Size::new(40, 6)).unwrap())
        .start();
    input.send(source("select **this** text")).unwrap();
    let _ = output.recv_timeout(Duration::from_secs(1)).unwrap();

    let down = crossterm::event::Event::Mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: 0,
        row: 0,
        modifiers: crossterm::event::KeyModifiers::empty(),
    });
    dispatcher.dispatch(down);
    let outcome: DispatchOutcome = dispatcher.dispatch(crossterm::event::Event::Key(
        crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::CONTROL,
        ),
    ));
    assert!(outcome.propagation_stopped);
}

#[allow(dead_code)]
fn public_component_signature(component: fn(&mut ComponentContext, &Props<MarkdownProps>) -> Node) {
    let _ = component;
}

#[test]
fn markdown_function_has_the_public_component_signature() {
    public_component_signature(markdown);
}
