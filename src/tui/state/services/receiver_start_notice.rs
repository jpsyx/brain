use super::AppServices;
use crate::server::delivery::ReceiverDeliveryExecutionPoll;

impl AppServices {
    pub(crate) fn tick_receiver_start_notice(
        &mut self,
        command: &crate::workspace::CommandContext,
    ) -> bool {
        match self.receiver_start_execution.poll() {
            ReceiverDeliveryExecutionPoll::Ready { result, .. } => {
                self.receiver_start_active = false;
                if !matches!(
                    result,
                    crate::state::ReceiverProviderResultClass::Acknowledged(_)
                ) {
                    crate::logging::log("receiver start notice was not acknowledged");
                }
            }
            ReceiverDeliveryExecutionPoll::Disconnected => self.receiver_start_active = false,
            ReceiverDeliveryExecutionPoll::Pending => {}
        }
        if self.receiver_start_active {
            return true;
        }
        let notice = match self.db.pending_receiver_start_notice() {
            Ok(Some(notice)) => notice,
            Ok(None) => return false,
            Err(error) => {
                crate::logging::log(format!("receiver start notice lookup failed: {error:#}"));
                return false;
            }
        };
        let Ok(start) = self
            .receiver_start_execution
            .reserve(command.clone(), notice.clone())
        else {
            return false;
        };
        match self.db.consume_receiver_start_notice(&notice) {
            Ok(true) => {}
            Ok(false) => return false,
            Err(error) => {
                crate::logging::log(format!("receiver start notice claim failed: {error:#}"));
                return false;
            }
        }
        if start.start().is_err() {
            crate::logging::log("receiver start notice worker publication failed");
            return false;
        }
        self.receiver_start_active = true;
        true
    }

    #[cfg(test)]
    pub(crate) fn replace_receiver_start_execution(
        &mut self,
        execution: Box<dyn crate::server::delivery::ReceiverDeliveryExecution>,
    ) {
        self.receiver_start_execution = execution;
    }
}
