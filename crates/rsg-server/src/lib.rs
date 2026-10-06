//! rsg-server library: the scheduler-boundary transport, handshake, single
//! ordered writer (D-01) and per-uid reply dispatcher (D-04/D-05/D-07)
//! shared by the rsg-server and mock-scheduler binaries.

pub mod dispatch;
pub mod handshake;
pub mod transport;
pub mod writer;
