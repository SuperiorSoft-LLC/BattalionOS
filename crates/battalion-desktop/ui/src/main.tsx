// SPDX-License-Identifier: MPL-2.0
import { createRoot } from "react-dom/client";
import "../kit/styles.css";
import { App } from "./App";
import { hostOs } from "./os";

createRoot(document.getElementById("root")!).render(<App os={hostOs} />);
