import React from "react";
import ReactDOM from "react-dom/client";
import "./styles.css";
import { Selector } from "./Selector";
import { Result } from "./Result";
import { Settings } from "./Settings";
import { History } from "./History";
const boot = window.__OVERTEXT__;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {boot?.view === "selector" && <Selector {...boot} />}
    {boot?.view === "result" && <Result {...boot} />}
    {boot?.view === "settings" && <Settings />}
    {boot?.view === "history" && <History />}
  </React.StrictMode>,
);
