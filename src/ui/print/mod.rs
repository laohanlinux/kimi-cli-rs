/// Print-mode UI renderer.
#[derive(Debug, Clone, Default)]
pub struct PrintUi;

impl PrintUi {
    pub fn render(&self, text: &str) {
        println!("{text}");
    }
}
