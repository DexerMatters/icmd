//! Inline hyperlink widget that reports its target on activation.

use crate::{Attr, DomProps, EventListener, Node, Props, Style, basic::ComponentContext};

use super::interactive::{Activation, interactive};

/// Configuration for [`link`].
///
/// A link is an inline interactive element: it paints like body text with the
/// link treatment (accent colour plus an underline) and reports its target when
/// activated. Following stays an application decision - the framework never
/// opens a browser, launches a process, or touches the network on its own.
#[derive(Clone, Default)]
pub struct LinkProps {
    /// The target reported on activation. It is also the visible text when the
    /// caller gives neither a label nor children, so a bare link reads as the
    /// URL it points at.
    pub href: Attr<String>,
    /// Visible text when the caller supplies no children; when empty, `href` is
    /// shown instead.
    pub label: Attr<String>,
    /// Whether activation and focus are refused; defaults to `false`.
    pub disabled: Attr<bool>,
    /// Whether the link requests focus on mount; defaults to `false`.
    pub autofocus: Attr<bool>,
    /// One follow request per activation, carrying the target. The listener
    /// slot is opaque, so it is omitted from the debug output.
    pub on_follow: Attr<EventListener<String>>,
}

impl std::fmt::Debug for LinkProps {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkProps")
            .field("href", &self.href)
            .field("label", &self.label)
            .field("disabled", &self.disabled)
            .field("autofocus", &self.autofocus)
            .finish_non_exhaustive()
    }
}

/// Inline hyperlink; see [`LinkProps`] for its configuration.
///
/// A disabled link keeps its place in the layout but loses the affordances
/// that invite a click: the accent role and the underline. The underline is
/// the non-colour affordance, so a link stays recognizable where the accent is
/// unavailable, ignored, or indistinguishable from body text. The listener
/// node owns the themed style, so it is the painted and hit-tested node rather
/// than a wrapper around one.
pub fn link(cx: &mut ComponentContext, props: &Props<LinkProps>) -> Node {
    let theme = cx.use_theme();
    let href = props.href.clone() | String::new();
    let disabled = props.disabled | false;

    let mut style = Style {
        text: theme.typography.body.clone(),
        ..Style::default()
    };
    style.text.foreground /= if disabled {
        theme.colors.muted_foreground
    } else {
        theme.colors.primary
    };
    style.text.attr.underlined /= !disabled;

    let children = if props.children.is_empty() {
        let label = props.label.clone() | String::new();
        vec![Node::from(if label.is_empty() {
            href.clone()
        } else {
            label
        })]
    } else {
        props.children.clone()
    };

    let on_follow = props.on_follow.as_ref().cloned();
    let activation = Activation::new(disabled, props.autofocus | false).on_activate(move || {
        if let Some(listener) = &on_follow {
            listener.call(href.clone());
        }
    });

    interactive(
        DomProps::default().with_style(style),
        &props.dom,
        activation,
        children,
    )
}
