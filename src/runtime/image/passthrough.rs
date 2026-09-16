//! The multiplexer passthrough kind: the only Chafa terminal-info state the
//! renderer needs after detection. Reducing it to a plain value means
//! `Renderer` retains no native pointer and needs no `Send` assertion for a
//! foreign object to cross into the renderer worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Passthrough {
    /// No multiplexer; escapes pass through unwrapped.
    #[default]
    None,
    /// tmux passthrough envelope.
    Tmux,
    /// GNU Screen passthrough envelope.
    Screen,
}

impl Passthrough {
    /// Wraps `command` in this multiplexer's passthrough envelope, or returns
    /// `None` when no wrapping is needed.
    pub(crate) fn escape(self, command: &str) -> Option<String> {
        match self {
            Self::None => None,
            Self::Tmux => Some(format!("\x1bPtmux;\x1b{command}\x1b\\")),
            Self::Screen => Some(format!("\x1bP{command}\x1b\\")),
        }
    }
}

#[cfg(feature = "native-raster")]
impl Passthrough {
    /// Converts a Chafa passthrough kind, mapping anything unknown to `None`.
    pub(crate) fn from_chafa(value: chafa_sys::ChafaPassthrough) -> Self {
        match value {
            chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_TMUX => Self::Tmux,
            chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_SCREEN => Self::Screen,
            _ => Self::None,
        }
    }

    /// Converts to the Chafa passthrough kind.
    pub(crate) fn to_chafa(self) -> chafa_sys::ChafaPassthrough {
        match self {
            Self::None => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_NONE,
            Self::Tmux => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_TMUX,
            Self::Screen => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_SCREEN,
        }
    }
}
