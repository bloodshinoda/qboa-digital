#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod capability_inventory;
mod cleanup_engine;
mod cleanup_findings;
mod cleanup_progress;
mod complete_diagnostic;
mod cpu_diagnostic;
mod diagnostics;
mod diagnostics_store;
mod gpu_diagnostic;
mod memory_inventory;
mod memory_test;
mod operation_contracts;
mod registry_executor;
mod safety;
mod service_manager;
mod service_profiles;
mod storage_diagnostic;
mod storage_integrity;
mod system_compat;
mod task_registry;
mod technical_inventory;
mod technical_inventory_collectors;
mod technical_inventory_storage;
mod tweak_profiles;
mod windows_update;
