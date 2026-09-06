pub mod basic;
pub mod data;
mod runtime;

pub use basic::{
    Component as ElementComponent, Context as ElementContext, DomId, DomNode, DomProps,
    EffectResult, EventListener, FocusEvent, Key, Node, Props, Ref as ElementRef, StateSetter,
};
pub use data::{
    Cell, CellEdit, Frame, Image, ImageError, ImageId, ImagePosition, Operation, Rect,
    ScreenPosition, Size,
};
pub use runtime::{Component, FrameError, Lower, Renderer, Runtime};

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use crossterm::style::{Attributes, Color};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn cell(symbol: &str) -> Cell {
        Cell::new(symbol, Color::Reset, Color::Reset, Attributes::default()).unwrap()
    }

    fn image(row: &[&str]) -> Image {
        Image::from_rows(vec![row.iter().map(|symbol| cell(symbol)).collect()]).unwrap()
    }

    #[test]
    fn initial_render_and_noop_are_incremental() {
        let mut renderer = Renderer::new(Size::new(4, 1));
        assert!(renderer.render_diff().unwrap().contains("\x1b[2J"));
        assert!(renderer.render_diff().is_none());

        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image: image(&["A"]),
                position: ScreenPosition::new(0, 1),
                level: 0,
            }]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('A'));
        assert!(renderer.render_diff().is_none());
    }

    #[test]
    fn invalid_frame_is_atomic() {
        let mut renderer = Renderer::new(Size::new(4, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image: image(&["A"]),
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        renderer.render_diff();

        let result = renderer.apply_frame(Frame::new(vec![
            Operation::Move {
                id: ImageId(99),
                position: ScreenPosition::default(),
            },
            Operation::Create {
                id: ImageId(2),
                image: image(&["B"]),
                position: ScreenPosition::new(0, 1),
                level: 0,
            },
        ]));
        assert!(matches!(result, Err(FrameError::UnknownImage(ImageId(99)))));
        assert!(renderer.render_diff().is_none());
    }

    #[test]
    fn overlap_uses_level_and_restores_underlying_image() {
        let mut renderer = Renderer::new(Size::new(4, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![
                Operation::Create {
                    id: ImageId(1),
                    image: image(&["A"]),
                    position: ScreenPosition::default(),
                    level: 0,
                },
                Operation::Create {
                    id: ImageId(2),
                    image: image(&["B"]),
                    position: ScreenPosition::default(),
                    level: 0,
                },
            ]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('B'));
        renderer
            .apply_frame(Frame::new(vec![Operation::Remove { id: ImageId(2) }]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('A'));
    }

    #[test]
    fn same_level_images_can_be_reordered_without_remounting() {
        let mut renderer = Renderer::new(Size::new(1, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![
                Operation::Create {
                    id: ImageId(1),
                    image: image(&["A"]),
                    position: ScreenPosition::default(),
                    level: 0,
                },
                Operation::Create {
                    id: ImageId(2),
                    image: image(&["B"]),
                    position: ScreenPosition::default(),
                    level: 0,
                },
            ]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('B'));

        renderer
            .apply_frame(Frame::new(vec![Operation::SetOrder {
                id: ImageId(1),
                order: 2,
            }]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('A'));
    }

    #[test]
    fn wide_cells_are_emitted_and_validated() {
        let wide = cell("界");
        assert_eq!(wide.width(), 2);
        let image = Image::from_rows(vec![vec![wide]]).unwrap();
        assert_eq!(image.width(), 2);
        let mut renderer = Renderer::new(Size::new(2, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image,
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains('界'));
        assert!(Cell::plain("ab").is_err());
    }

    #[test]
    fn patching_a_wide_glyph_boundary_is_safe() {
        let wide = cell("界");
        let mut image = Image::from_rows(vec![vec![wide, cell("A")]]).unwrap();
        let patch = Operation::PatchRect {
            id: ImageId(1),
            rect: Rect::new(0, 1, 1, 1),
            rows: vec![vec![cell("B")]],
        };
        let mut renderer = Renderer::new(Size::new(3, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image: image.clone(),
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        renderer.render_diff();
        renderer.apply_frame(Frame::new(vec![patch])).unwrap();
        assert!(renderer.render_diff().unwrap().contains('B'));
        image
            .patch_cells(&[CellEdit {
                position: ImagePosition::new(0, 1),
                cell: cell("C"),
            }])
            .unwrap();
    }

    #[test]
    fn canvas_patches_and_resize_damage_the_right_regions() {
        let mut renderer = Renderer::new(Size::new(4, 1));
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image: image(&["A", "B", "C", "D"]),
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        renderer.render_diff();
        renderer
            .apply_frame(Frame::new(vec![Operation::PatchRect {
                id: ImageId(1),
                rect: Rect::new(0, 1, 1, 1),
                rows: vec![vec![cell("Z")]],
            }]))
            .unwrap();
        let patch = renderer.render_diff().unwrap();
        assert!(patch.contains('Z'));
        assert!(!patch.contains('A'));

        renderer
            .apply_frame(Frame::empty().resize(Size::new(2, 1)))
            .unwrap();
        assert!(renderer.render_diff().unwrap().contains("\x1b[2J"));
    }

    #[test]
    fn pipeline_emits_diffs_and_rejections() {
        let (input, output) = Runtime::with_capacity(Renderer::new(Size::new(4, 1)), 4).start();
        input
            .send(Frame::new(vec![Operation::Create {
                id: ImageId(1),
                image: image(&["A"]),
                position: ScreenPosition::default(),
                level: 0,
            }]))
            .unwrap();
        input
            .send(Frame::new(vec![Operation::Move {
                id: ImageId(99),
                position: ScreenPosition::default(),
            }]))
            .unwrap();
        let mut saw_diff = false;
        let mut saw_rejection = false;
        for _ in 0..3 {
            if let Ok(result) = output.recv_timeout(std::time::Duration::from_secs(1)) {
                match result {
                    Ok(value) => saw_diff |= value.contains('A'),
                    Err(_) => saw_rejection = true,
                }
                if saw_diff && saw_rejection {
                    break;
                }
            }
        }
        assert!(saw_diff && saw_rejection);
    }

    #[test]
    fn runtime_connects_typed_components() {
        struct AddOne;
        impl Component for AddOne {
            type Input = u32;
            type Output = u32;

            fn run(
                self,
                input: crossbeam_channel::Receiver<Self::Input>,
                output: crossbeam_channel::Sender<Self::Output>,
            ) {
                while let Ok(value) = input.recv() {
                    if output.send(value + 1).is_err() {
                        break;
                    }
                }
            }
        }

        struct ToText;
        impl Component for ToText {
            type Input = u32;
            type Output = String;

            fn run(
                self,
                input: crossbeam_channel::Receiver<Self::Input>,
                output: crossbeam_channel::Sender<Self::Output>,
            ) {
                while let Ok(value) = input.recv() {
                    if output.send(value.to_string()).is_err() {
                        break;
                    }
                }
            }
        }

        let (input, output) = Runtime::new(AddOne).then(ToText).start();
        input.send(41).unwrap();
        assert_eq!(
            output
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap(),
            "42"
        );
    }

    #[test]
    fn lower_preserves_dom_event_listeners() {
        struct View;

        impl ElementComponent for View {
            type Props = ();

            fn render(_cx: &mut ElementContext, _props: &Props<Self::Props>) -> Node {
                Node::empty()
            }
        }

        let calls = Arc::new(AtomicUsize::new(0));
        let props = Props::new(())
            .on_mouse_event({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                }
            })
            .on_keyboard_event({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                }
            })
            .on_resize_event({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                }
            })
            .on_focus_event({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                }
            })
            .on_paste_event({
                let calls = calls.clone();
                move |_| {
                    calls.fetch_add(1, Ordering::Relaxed);
                }
            });

        let (input, output) = Runtime::new(Lower::default()).start();
        input.send(Node::component::<View>(props)).unwrap();
        let dom = output
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        let DomNode::Element { props, .. } = dom else {
            panic!("component did not lower to an element");
        };

        props.mouse_event.as_ref().unwrap().call(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        });
        props
            .keyboard_event
            .as_ref()
            .unwrap()
            .call(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        props.resize_event.as_ref().unwrap().call(Size::new(80, 24));
        props.focus_event.as_ref().unwrap().call(FocusEvent::Gained);
        props
            .paste_event
            .as_ref()
            .unwrap()
            .call(String::from("hello"));

        assert_eq!(calls.load(Ordering::Relaxed), 5);
    }
}
