use crate::{Result, Shell, Error};
use alloc::vec::Vec;

// TODO: Decide which internal commands we don't need.

impl Shell {
    pub(crate) fn alias(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn bg(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn cd(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn exec(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn exit(&self, args: Vec<&str>) -> Result<()> {
        // TODO: Clean up background tasks?
        Err(Error::Exit)
    }

    pub(crate) fn export(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn fc(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn fg(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn getopts(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn hash(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn set(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn unalias(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn unset(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }

    pub(crate) fn wait(&self, args: Vec<&str>) -> Result<()> {
        todo!();
    }
}
