//! Exactly one prepared setup at the managed compatibility boundary.
use anyhow::Result;

pub(super) struct Preparation<'a> {
    callback: Option<Box<dyn FnOnce() -> Result<()> + 'a>>,
}
impl<'a> Preparation<'a> {
    pub(super) fn new(callback: impl FnOnce() -> Result<()> + 'a) -> Self {
        Self {
            callback: Some(Box::new(callback)),
        }
    }
    pub(super) fn pending(&self) -> bool {
        self.callback.is_some()
    }
    pub(super) fn run(&mut self) -> Result<()> {
        self.callback.take().map_or(Ok(()), |callback| callback())
    }
}
