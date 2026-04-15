/// Runtime for evaluating approval rules.
#[derive(Debug, Clone, Default)]
pub struct ApprovalRuntime;

impl ApprovalRuntime {
    pub fn bind_root_wire_hub(&self, _root_wire_hub: &crate::wire::root_hub::RootWireHub) {}
}
