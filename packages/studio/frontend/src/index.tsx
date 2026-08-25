import "@xterm/xterm/css/xterm.css"
// startr.style first, then our layer — the same order Sage.is AI-UI uses
// (framework, then custom.css). Ours overrides; never the reverse.
import "./startr.style.css"
import { render } from "solid-js/web"
import App from "./App"
import "./styles.css"

render(() => <App />, document.getElementById("root")!)
