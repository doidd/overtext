import { createElement } from "react";
import { createRoot } from "react-dom/client";
import { Result } from "../../src/Result";

createRoot(document.getElementById("root")!).render(createElement(Result, { imagePath: "capture.png", width: 408, height: 204 }));
