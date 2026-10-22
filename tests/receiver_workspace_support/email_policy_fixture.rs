use super::SharedReceiverFixture;

impl SharedReceiverFixture {
    pub fn configure_email_policy(&self, policy: serde_json::Value, recipient: &str) {
        let store = brain::workspace::RegistryStore::from_path(
            self.home.path().join(".config/brain/env.json"),
        );
        let mut registry = brain::workspace::RegistryStore::load_from(store.path()).unwrap();
        registry
            .env
            .insert("resend_recipient_domain".to_owned(), policy);
        registry
            .workspaces
            .get_mut(self.workspace.name())
            .unwrap()
            .env
            .insert("resend_from_email".to_owned(), serde_json::json!(recipient));
        store.replace(&registry).unwrap();
    }
}
