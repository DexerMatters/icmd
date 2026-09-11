use crossterm::style::Color;
use icmd::{
    BorderKind, Commit, Component, ComponentContext, Edges, Lower, Node, PointerEvent, Renderer,
    Runtime, Size, paragraph,
};
use std::time::Duration;

fn button(cx: &mut ComponentContext, _: &icmd::Props<()>) -> Node {
    let (count, set_count) = cx.use_state(|| 0);
    let (pressed, set_pressed) = cx.use_state(|| false);

    paragraph
        .style(|s| {
            s.padding /= Edges::symmetric(1, 2);
            s.border.kind /= BorderKind::Rounded;
            s.background /= if pressed { Color::Blue } else { Color::Yellow };
            s.text.attr.bold /= pressed;
        })
        .events(move |e| {
            let set_pressed_down = set_pressed.clone();
            e.pointer_down /= move |_event: PointerEvent| set_pressed_down.set(true);
            e.pointer_up /= move |_event: PointerEvent| {
                set_pressed.set(false);
                set_count.set(count + 1);
            };
        })
        .child(format!("count: {}", count))
}

#[test]
fn button_renders() {
    let viewport = Size::new(40, 4);
    let (commit, _) = Commit::new(viewport);
    let (input, output) = Runtime::new(Lower::default())
        .then(commit)
        .then(Renderer::new(viewport).unwrap())
        .start();
    input.send(button.apply(())).unwrap();

    let frame = output
        .recv_timeout(Duration::from_secs(1))
        .expect("runtime did not produce a frame")
        .expect("renderer failed");
    assert!(frame.contains('╭'));
}
