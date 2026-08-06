use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::super::{SharedReceiverPipeline, execute_pipeline};
use crate::server::control::{ControlRequest, ControlResponse, ControlServer, LeaseRegistration};
use crate::server::lifecycle::{IngressId, LeaseId, ServerGeneration};
use crate::server::receiver::Channel;
use crate::server::receiver::admission::ReceiverAdmission;
use crate::workspace::{
    MachineRegistry, RegistryStore, WorkspaceContext, WorkspaceId, WorkspaceName, WorkspaceRecord,
};

const PERSONAL_ID: &str = "8ccd7c41-1b6e-4a3c-b91e-1b0117b77a2b";

#[derive(Clone, Copy)]
enum LateRevocation {
    Disable,
    Unregister,
    DisableEnableAba,
    Expire,
    RouteLookupThenExpire,
    ExpireBeforeCommitWithoutWatchdog,
    ExpireDuringCommitIntentReload,
    ExpireWhileCommitWaitsForControl,
    CommitLinearizesUnderControl,
}

