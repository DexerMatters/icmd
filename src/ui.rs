//! JSX-inspired declarative UI composition.
//!
//! The [`ui!`] macro is intentionally a small Rust-native view syntax rather
//! than an HTML implementation. Tags name imported `Component` values,
//! ordinary attributes write to the component's `Attr<T>` props, and children
//! are converted to [`crate::Node`] values.

/// Build a [`crate::Node`] tree using JSX-inspired syntax.
///
/// Components are referenced by imported identifiers. A component may be
/// written as a paired element, a self-closing element, or a fragment:
///
/// ```
/// # use icmd::{button, header, progress_bar, ui, vbox};
/// # fn example(done: u64, total: u64, caption: String) {
/// #     let submit = || {};
/// let _node = ui! {
///     <vbox>
///         <header>"Status"</header>
///         <progress_bar value={done} max={total} label="build" />
///         <button on_click={move |_| submit()}>{caption}</button>
///     </vbox>
/// };
/// # }
/// ```
///
/// `dom={...}` forwards a complete [`crate::DomProps`] value, while
/// `style={|style| ...}` and `events={|events| ...}` mutate the DOM props;
/// `on_click`, `on_key_down`, and the other `EventHandlers` fields install a
/// single listener. `key` is applied to the resulting node. Ordinary
/// attributes assign to fields on the component's user-defined props.
#[macro_export]
macro_rules! ui {
    // A fragment opening frame.
    (@parse [$($stack:tt)*] [$($nodes:tt)*] < > $($rest:tt)*) => {
        $crate::ui!(@parse [[<> [$($nodes)*]] $($stack)*] [] $($rest)*)
    };

    // A fragment closing frame.
    (@parse [[<> [$($saved:tt)*]] $($stack:tt)*] [$($children:tt)*] < / > $($rest:tt)*) => {
        $crate::ui!(@append_frame [$($stack)*] [$($saved)*] ($crate::fragment::<_, $crate::Node>(vec![$($children)*])) $($rest)*)
    };

    // Close an element frame. A const string comparison below gives matching
    // identifiers a focused compile-time diagnostic.
    (@parse [[$tag:ident [$($attrs:tt)*] [$($saved:tt)*]] $($stack:tt)*] [$($children:tt)*] < / $close:ident > $($rest:tt)*) => {
        $crate::ui!(@check_close $tag $close {
            $crate::ui!(@append_frame [$($stack)*] [$($saved)*]
                ($crate::ui!(@build $tag [$($attrs)*] [$($children)*])) $($rest)*)
        })
    };

    // An element opening frame is parsed separately so `>` and `/>` can be
    // recognized without trying to parse arbitrary Rust tokens.
    (@parse [$($stack:tt)*] [$($nodes:tt)*] < $tag:ident $($rest:tt)*) => {
        $crate::ui!(@open [$($stack)*] [$($nodes)*] $tag [] $($rest)*)
    };

    // Child expressions and string literals.
    (@parse [$($stack:tt)*] [$($nodes:tt)*] { $expr:expr } $($rest:tt)*) => {
        $crate::ui!(@parse [$($stack)*] [$($nodes)* ($expr).into(),] $($rest)*)
    };
    (@parse [$($stack:tt)*] [$($nodes:tt)*] $text:literal $($rest:tt)*) => {
        $crate::ui!(@parse [$($stack)*] [$($nodes)* ($text).into(),] $($rest)*)
    };

    // End of the root tree. Preserve a single root component so it can be
    // sent directly to the runtime; multiple roots and empty input become
    // fragments.
    (@parse [] []) => {
        $crate::fragment::<_, $crate::Node>(vec![])
    };
    (@parse [] [($single:expr),]) => {
        $single
    };
    (@parse [] [$($nodes:tt)+]) => {
        $crate::fragment::<_, $crate::Node>(vec![$($nodes)+])
    };

    // A closing tag without an opening frame, or an unclosed frame, is a
    // syntax error. Mismatched names are diagnosed by @check_close above.
    (@parse [] [$($nodes:tt)*] < / $($rest:tt)*) => {
        compile_error!("ui!: unexpected closing tag")
    };
    (@parse [[$tag:ident [$($attrs:tt)*] [$($saved:tt)*]] $($stack:tt)*] [$($nodes:tt)*]) => {
        compile_error!(concat!("ui!: missing closing tag for <", stringify!($tag), ">"))
    };
    (@parse [$($stack:tt)*] [$($nodes:tt)*] $($rest:tt)*) => {
        compile_error!("ui!: expected an element, fragment, string literal, or {expression}")
    };

    // Opening-tag parser: self-closing elements and paired elements.
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

    // Pop a frame and append its completed node to the saved parent children.
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

    // Match closing identifiers in a const context so a mismatch gets a
    // stable, focused diagnostic while remaining entirely declarative.
    (@check_close $expected:ident $actual:ident { $($continuation:tt)* }) => {{
        const _: () = if !$crate::__ui_tag_names_equal(
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

    // Build one component after the parser has collected its attributes and
    // children. The component and props types are inferred by `apply`.
    (@build $component:ident [$($attrs:tt)*] [$($children:tt)*]) => {{
        $crate::__ui_apply($component, move |__icmd_ui_props| {
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
        $props.dom.events = $crate::__ui_events($value);
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
    (@set_attr $props:ident $key:ident on_key_down { $value:expr }) => { $props.dom.events.key_down /= $value; };
    (@set_attr $props:ident $key:ident on_key_up { $value:expr }) => { $props.dom.events.key_up /= $value; };
    (@set_attr $props:ident $key:ident on_keyboard_event { $value:expr }) => { $props.dom.events.keyboard_event /= $value; };
    (@set_attr $props:ident $key:ident on_resize_event { $value:expr }) => { $props.dom.events.resize_event /= $value; };
    (@set_attr $props:ident $key:ident on_focus_event { $value:expr }) => { $props.dom.events.focus_event /= $value; };
    (@set_attr $props:ident $key:ident on_paste_event { $value:expr }) => { $props.dom.events.paste_event /= $value; };

    (@set_attr $props:ident $key:ident $name:ident { $value:expr }) => {
        $props.user_defined.$name /= $value;
    };

    ($($tokens:tt)*) => {
        $crate::ui!(@parse [] [] $($tokens)*)
    };
}
