// SPDX-License-Identifier: MPL-2.0
//! The OS ends (Exit). Every running app is stopped cleanly first: the app's `stop`, the app's
//! stop timeout, then its job. Then the OS goes. (Closing the window is not an exit: apps run on.)

use super::*;

impl Armory {
    /// Stop every running app, then return. Returns (id, error) of an app that could not be
    /// stopped; the exit goes on regardless (the OS's end closes every job anyway).
    pub fn exit_os(&self) -> Vec<(String, String)> {
        self.sup.stop_all()
    }
}
