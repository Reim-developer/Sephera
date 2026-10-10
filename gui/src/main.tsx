import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./styles/theme.css";

const container = document.getElementById("root");
if (!container) {
  throw new Error("index.html has no #root for the React tree to mount into");
}

// StrictMode is on in development and off in production, which is the default
// `createRoot` behaviour. The double-invoked effects it adds in development are
// why every effect below writes its results through a cancellation flag rather
// than assuming it runs once.
createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
