/// BTW (By The Way) notification injector.
#[derive(Debug, Clone, Default)]
pub struct BtwNotifier;

impl BtwNotifier {
    /// Checks if a BTW notification should be injected into the current turn.
    pub fn should_notify(&self, _context: &crate::soul::context::Context) -> Option<String> {
        None
    }
}
