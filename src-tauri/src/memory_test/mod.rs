//! Núcleo isolado do QBoa Memory Test.
//!
//! Os algoritmos operam somente sobre slices próprios do processo. O runner
//! controla orçamento, alocação adaptativa, cancelamento cooperativo e progresso.
//! A integração com eventos Tauri e a persistência do laudo ficam na camada externa.

pub mod algorithms;
pub mod runner;
