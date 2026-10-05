import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { Settings } from "../../src/Settings";

createRoot(document.getElementById("root")!).render(createElement(Settings));
