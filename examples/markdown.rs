use std::error::Error;

use icmd::{
    Component, ComponentContext, Node, RuntimeConfig, markdown, render,
    theme::{ThemeMode, ThemePreset},
    theme_provider, ui,
};

fn app(_cx: &mut ComponentContext, _props: &icmd::Props<()>) -> Node {
    ui! {
        <theme_provider value={ThemePreset::Nord.theme(ThemeMode::Dark)}>
            <markdown text={r#"
# Hello

This is **selectable** Markdown.

## Heading

* Item 1
* Item 2

### Subheading

#### Sub-subheading

"#} />
        </theme_provider>
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    render(app.apply(()), RuntimeConfig::default())?;
    Ok(())
}
