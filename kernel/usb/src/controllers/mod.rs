use super::*;

pub mod ehci;

pub enum Controller<T> {
    Ehci(T),
}
