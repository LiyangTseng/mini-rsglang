//! rsg-server library: the scheduler-boundary transport, handshake, single
//! ordered writer (D-01) and per-uid reply dispatcher (D-04/D-05/D-07)
//! shared by the rsg-server and mock-scheduler binaries; the request
//! lifecycle engine (`engine`), the tokenizer/detokenizer seam (`codec`),
//! and the HTTP ingress layer (`http`) added in Phase 5.

pub mod codec;
pub mod dispatch;
pub mod engine;
pub mod fsm;
pub mod handshake;
pub mod http;
pub mod metrics;
pub mod transport;
pub mod writer;
