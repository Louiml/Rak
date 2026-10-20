# Native GUI

Rak programs can open desktop windows. Requires
`cargo build -p rakc --features gui`. Works on Windows and Linux.

The GUI is **native**: windows and widgets are drawn by `egui` through the
`glow` renderer. There is no HTML, no CSS, no JavaScript, and no WebView2 or
WebKitGTK anywhere in the pipeline, so a Rak GUI app does not need a browser
engine installed and does not depend on the host's web stack.

```rak
fn on_click(name, message) {
  print f"{name} says: {message}"
  return nil
}

gui_callback("on_widget", on_click)

let win = gui_window("Rak GUI Demo", 420, 240)

let heading = gui_add(win, "label", "Hello from Rak!", 20, 20, 380, 28)
let name = gui_add(win, "input", "world", 20, 70, 240, 28)
let agree = gui_add(win, "checkbox", "I agree", 20, 120, 200, 24)
let button = gui_add(win, "button", "Click me", 20, 170, 140, 34)
gui_add(win, "select", "Small, Medium, Large", 20, 210, 200, 28)

gui_on_click(button, "on_widget")
gui_set_text(heading, "Native widgets, zero HTML")

dump f"input reads: {gui_get_text(name)}"

gui_wait()
```

## The builtins

| Builtin | Behaviour |
|---|---|
| `gui_window(title, w?, h?)` | Opens a window and returns its id. It blocks until the window exists, so the id is real. |
| `gui_add(window, kind, text, x?, y?, w?, h?)` | Adds a widget at an absolute position inside a window; returns the widget id. |
| `gui_set_text(widget, text)` | Sets a caption, or an input's current text. |
| `gui_get_text(widget)` | Reads a widget back: the input's text, `"true"`/`"false"` for a checkbox, otherwise the caption. |
| `gui_set_visible(widget, bool)` | Shows or hides a widget without removing it. |
| `gui_on_click(widget, name)` | Binds a widget to a callback registered under `name`. |
| `gui_close(id)` | Closes one window. The process keeps running. |
| `gui_wait()` | Blocks until every window is closed. |
| `gui_quit(code?)` | Stops the GUI with an exit code. |
| `gui_callback(name, fn)` | Registers a Rak function that `gui_on_click` dispatches to. |

Widget kinds are `label`, `button`, `input`, `checkbox`, `select` and
`textarea`. A `select` takes its options as one comma-separated string.

Both backends work: the VM has GUI natives, and they resolve the same
process-wide manager the interpreter does.

## The designer

The IDE's **GUI Designer** — in the toolbar, or through the command palette —
draws a window visually and emits this native Rak code, so a designed window
is the same code you would write by hand.

## One real limitation

A callback does not share mutable state with the script that registered it. It
receives a copy of the environment as it stood at `gui_callback` time, so it
can read what existed then, but it cannot write to a variable the main script
reads afterwards.

That follows from Rak having no reference types: values are shared rather
than moved, and a closure sees its enclosing frame by value. See
[Language reference](language.html#closures).

Also worth knowing: a function body without an explicit `return` evaluates to
`nil`, which matters here because `gui_callback` takes a function.

## The threading

The event loop runs on the main thread, on both platforms: the windowing
backend binds to the display connection from whichever thread creates the
loop and rejects any other. A callback therefore runs on the main thread,
between frames.

## Renamed in 1.0.1

The webview-era builtins `gui_open`, `gui_update` and `gui_title` are gone.
`gui_open(title, html, w, h)` became `gui_window(title, w, h)` followed by
`gui_add(...)` calls; `gui_update` and `gui_title` have no native equivalent —
use `gui_set_text` and `gui_set_visible`. Scripts written against the old
HTML API will not run unchanged.
