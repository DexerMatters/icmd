use icmd::ui;

mod widgets {
    pub fn status(_: &mut icmd::ComponentContext, _: &icmd::Props<()>) -> icmd::Node {
        "status".into()
    }
}

fn main() {
    let _ = ui! { <widgets::status /> };
}
