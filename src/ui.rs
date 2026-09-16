//! The `ui!` macro: a declarative element syntax that expands to ordinary
//! component calls and [`Node`](crate::Node) construction.

/// Build a [`Node`](crate::Node) tree with element syntax.
///
/// Each tag is a component or element constructor; attributes become props, and
/// `{expr}` splices a child. Component tags are ordinary components, so a
/// user-defined one is written exactly like a built-in.
///
/// ```
/// use icmd::ui;
/// use icmd::widgets::{button, column};
///
/// let node = ui! {
///     <column>
///         "hello"
///         <button on_press={|_| {}}>"press"</button>
///     </column>
/// };
/// let _ = node;
/// ```
#[macro_export]
macro_rules! ui {
    (@parse [$($stack:tt)*] [$($nodes:tt)*] < > $($rest:tt)*) => {
        $crate::ui!(@parse [[<> [$($nodes)*]] $($stack)*] [] $($rest)*)
    };

    (@parse [[<> [$($saved:tt)*]] $($stack:tt)*] [$($children:tt)*] < / > $($rest:tt)*) => {
        $crate::ui!(@append_frame [$($stack)*] [$($saved)*] ($crate::fragment::<_, $crate::Node>(vec![$($children)*])) $($rest)*)
    };

    (@parse [[$tag:ident [$($attrs:tt)*] [$($saved:tt)*]] $($stack:tt)*] [$($children:tt)*] < / $close:ident > $($rest:tt)*) => {
        $crate::ui!(@check_close $tag $close {
            $crate::ui!(@append_frame [$($stack)*] [$($saved)*]
                ($crate::ui!(@build $tag [$($attrs)*] [$($children)*])) $($rest)*)
        })
    };

    (@parse [$($stack:tt)*] [$($nodes:tt)*] < $tag:ident $($rest:tt)*) => {
        $crate::ui!(@open [$($stack)*] [$($nodes)*] $tag [] $($rest)*)
    };

    (@parse [$($stack:tt)*] [$($nodes:tt)*] { $expr:expr } $($rest:tt)*) => {
        $crate::ui!(@parse [$($stack)*] [$($nodes)* ($expr).into(),] $($rest)*)
    };
    (@parse [$($stack:tt)*] [$($nodes:tt)*] $text:literal $($rest:tt)*) => {
        $crate::ui!(@parse [$($stack)*] [$($nodes)* ($text).into(),] $($rest)*)
    };

    (@parse [] []) => {
        $crate::fragment::<_, $crate::Node>(vec![])
    };
    (@parse [] [($single:expr),]) => {
        $single
    };
    (@parse [] [$($nodes:tt)+]) => {
        $crate::fragment::<_, $crate::Node>(vec![$($nodes)+])
    };

    (@parse [] [$($nodes:tt)*] < / $($rest:tt)*) => {
        compile_error!("ui!: unexpected closing tag")
    };
    (@parse [[$tag:ident [$($attrs:tt)*] [$($saved:tt)*]] $($stack:tt)*] [$($nodes:tt)*]) => {
        compile_error!(concat!("ui!: missing closing tag for <", stringify!($tag), ">"))
    };
    (@parse [$($stack:tt)*] [$($nodes:tt)*] $($rest:tt)*) => {
        compile_error!("ui!: expected an element, fragment, string literal, or {expression}")
    };

    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] / > $($rest:tt)*) => {
        $crate::ui!(@parse [$($stack)*] [$($nodes)* ($crate::ui!(@build $tag [$($attrs)*] [])),] $($rest)*)
    };
    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] > $($rest:tt)*) => {
        $crate::ui!(@parse [[$tag [$($attrs)*] [$($nodes)*]] $($stack)*] [] $($rest)*)
    };
    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] $name:ident = { $value:expr } $($rest:tt)*) => {
        $crate::ui!(@open [$($stack)*] [$($nodes)*] $tag [$($attrs)* $name = { $value }] $($rest)*)
    };
    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] $name:ident = $value:literal $($rest:tt)*) => {
        $crate::ui!(@open [$($stack)*] [$($nodes)*] $tag [$($attrs)* $name = { $value }] $($rest)*)
    };
    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] $name:ident $($rest:tt)*) => {
        $crate::ui!(@open [$($stack)*] [$($nodes)*] $tag [$($attrs)* $name = { true }] $($rest)*)
    };
    (@open [$($stack:tt)*] [$($nodes:tt)*] $tag:ident [$($attrs:tt)*] $($rest:tt)*) => {
        compile_error!("ui!: malformed opening tag")
    };

    (@append_frame [] [$($saved:tt)*] ($node:expr) $($rest:tt)*) => {
        $crate::ui!(@parse [] [$($saved)* ($node),] $($rest)*)
    };
    (@append_frame [[$parent:ident [$($attrs:tt)*] [$($saved:tt)*]] $($stack:tt)*]
        [$($parent_nodes:tt)*] ($node:expr) $($rest:tt)*) => {
        $crate::ui!(@parse [[$parent [$($attrs)*] [$($saved)*]] $($stack)*]
            [$($parent_nodes)* ($node),] $($rest)*)
    };
    (@append_frame [[<> [$($saved:tt)*]] $($stack:tt)*]
        [$($parent_nodes:tt)*] ($node:expr) $($rest:tt)*) => {
        $crate::ui!(@parse [[<> [$($saved)*]] $($stack)*]
            [$($parent_nodes)* ($node),] $($rest)*)
    };

    (@check_close $expected:ident $actual:ident { $($continuation:tt)* }) => {{
        const _: () = if !$crate::__private::__ui_tag_names_equal(
            stringify!($expected),
            stringify!($actual),
        ) {
            panic!(concat!(
                "ui!: closing tag </",
                stringify!($actual),
                "> does not match opening tag <",
                stringify!($expected),
                ">",
            ));
        };
        $($continuation)*
    }};

    (@build $component:ident [$($attrs:tt)*] [$($children:tt)*]) => {{
        $crate::__private::__ui_apply($component, move |__icmd_ui_props| {
            let mut __icmd_ui_key: Option<$crate::Key> = None;
            $crate::ui!(@attrs __icmd_ui_props __icmd_ui_key; $($attrs)*);
            __icmd_ui_props.children = vec![$($children)*];
            __icmd_ui_key
        })
    }};

    (@attrs $props:ident $key:ident;) => {};
    (@attrs $props:ident $key:ident; $name:ident = { $value:expr } $($rest:tt)*) => {
        $crate::ui!(@set_attr $props $key $name { $value });
        $crate::ui!(@attrs $props $key; $($rest)*)
    };
    (@attrs $props:ident $key:ident; $name:ident = $value:literal $($rest:tt)*) => {
        $crate::ui!(@set_attr $props $key $name { $value });
        $crate::ui!(@attrs $props $key; $($rest)*)
    };
    (@attrs $props:ident $key:ident; $name:ident $($rest:tt)*) => {
        $crate::ui!(@set_attr $props $key $name { true });
        $crate::ui!(@attrs $props $key; $($rest)*)
    };

    (@set_attr $props:ident $key:ident key { $value:expr }) => {
        $key = Some(($value).into());
    };
    (@set_attr $props:ident $key:ident dom { $value:expr }) => {
        $props.dom = ($value).clone();
    };
    (@set_attr $props:ident $key:ident style { $value:expr }) => {
        $props.dom.style = $crate::style($value);
    };
    (@set_attr $props:ident $key:ident events { $value:expr }) => {
        $props.dom.events = $crate::__private::__ui_events($value);
    };

    (@set_attr $props:ident $key:ident on_pointer_down { $value:expr }) => { $props.dom.events.pointer_down /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_up { $value:expr }) => { $props.dom.events.pointer_up /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_move { $value:expr }) => { $props.dom.events.pointer_move /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_cancel { $value:expr }) => { $props.dom.events.pointer_cancel /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_over { $value:expr }) => { $props.dom.events.pointer_over /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_out { $value:expr }) => { $props.dom.events.pointer_out /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_enter { $value:expr }) => { $props.dom.events.pointer_enter /= $value; };
    (@set_attr $props:ident $key:ident on_pointer_leave { $value:expr }) => { $props.dom.events.pointer_leave /= $value; };
    (@set_attr $props:ident $key:ident on_got_pointer_capture { $value:expr }) => { $props.dom.events.got_pointer_capture /= $value; };
    (@set_attr $props:ident $key:ident on_lost_pointer_capture { $value:expr }) => { $props.dom.events.lost_pointer_capture /= $value; };
    (@set_attr $props:ident $key:ident on_click { $value:expr }) => { $props.dom.events.click /= $value; };
    (@set_attr $props:ident $key:ident on_wheel { $value:expr }) => { $props.dom.events.wheel /= $value; };
    (@set_attr $props:ident $key:ident on_scroll { $value:expr }) => { $props.dom.events.scroll /= $value; };
    (@set_attr $props:ident $key:ident on_key_down { $value:expr }) => { $props.dom.events.key_down /= $value; };
    (@set_attr $props:ident $key:ident on_key_up { $value:expr }) => { $props.dom.events.key_up /= $value; };
    (@set_attr $props:ident $key:ident on_keyboard_event { $value:expr }) => { $props.dom.events.keyboard_event /= $value; };
    (@set_attr $props:ident $key:ident on_app_key { $value:expr }) => { $props.dom.events.app_key /= $value; };
    (@set_attr $props:ident $key:ident on_resize_event { $value:expr }) => { $props.dom.events.resize_event /= $value; };
    (@set_attr $props:ident $key:ident on_focus_event { $value:expr }) => { $props.dom.events.focus_event /= $value; };
    (@set_attr $props:ident $key:ident on_terminal_focus { $value:expr }) => { $props.dom.events.terminal_focus /= $value; };
    (@set_attr $props:ident $key:ident on_paste_event { $value:expr }) => { $props.dom.events.paste_event /= $value; };

    (@set_attr $props:ident $key:ident $name:ident { $value:expr }) => {
        $props.data_mut().$name /= $value;
    };

    ($($tokens:tt)*) => {
        $crate::ui!(@parse [] [] $($tokens)*)
    };
}
