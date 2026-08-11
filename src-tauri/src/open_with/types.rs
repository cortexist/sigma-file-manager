// SPDX-License-Identifier: GPL-3.0-or-later
// License: GNU GPLv3 or later. See the license file in the project root for more information.
// Copyright © 2021 - present Aleksey Hoffman. All rights reserved.

use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AssociatedProgram {
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpenWithResult {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetAssociatedProgramsResult {
    pub success: bool,
    pub recommended_programs: Vec<AssociatedProgram>,
    pub other_programs: Vec<AssociatedProgram>,
    pub default_program: Option<AssociatedProgram>,
    pub error: Option<String>,
}

