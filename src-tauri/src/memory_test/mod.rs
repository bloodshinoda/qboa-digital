//! Núcleo isolado do QBoa Memory Test.
//!
//! Os algoritmos recebem apenas slices pertencentes ao processo. A integração
//! com alocação, cancelamento, eventos Tauri e persistência fica fora deste módulo.

pub mod algorithms;
