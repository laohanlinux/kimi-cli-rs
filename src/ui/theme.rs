/// Terminal theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "dark".into(),
        }
    }
}
