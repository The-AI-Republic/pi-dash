//! Runner runs services (D-15, stage 5).
//!
//! * [`chat`] — the chat service closure (`services/chat.py`, L5,
//!   PIDASHCONV-537). The L3 guards/shapes (PIDASHCONV-529) and L4
//!   lifecycle (PIDASHCONV-534) modules join this directory when
//!   they land.

pub mod chat;
