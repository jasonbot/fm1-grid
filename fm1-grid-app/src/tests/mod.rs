//! Headless UI smoke tests, split by feature. Shared window/store setup lives in
//! `support`; each test module keeps its own imports.

mod support;

mod devices;
mod library;
mod power;
mod settings;
mod window;
