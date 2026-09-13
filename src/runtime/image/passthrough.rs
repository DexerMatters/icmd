// The only piece of Chafa terminal-info state the renderer needs after
// detection is the multiplexer passthrough kind. Reducing it to a plain value
// means no native pointer is retained by `Renderer`, so no `Send` assertion is
// needed for a foreign object to cross into the renderer worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Passthrough {
    #[default]
    None,
    Tmux,
    Screen,
}

impl Passthrough {
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
    pub(crate) fn from_chafa(value: chafa_sys::ChafaPassthrough) -> Self {
        match value {
            chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_TMUX => Self::Tmux,
            chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_SCREEN => Self::Screen,
            _ => Self::None,
        }
    }

    pub(crate) fn to_chafa(self) -> chafa_sys::ChafaPassthrough {
        match self {
            Self::None => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_NONE,
            Self::Tmux => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_TMUX,
            Self::Screen => chafa_sys::ChafaPassthrough_CHAFA_PASSTHROUGH_SCREEN,
        }
    }
}
