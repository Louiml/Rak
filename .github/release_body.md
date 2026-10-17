## 1.0.1 - 2026-10-17

Rak's GUI was a WebView. A program handed `gui_open` a string of HTML, and the
page called back into Rak through injected JavaScript. That put a browser engine
in the path of every GUI app: WebView2 on Windows, WebKitGTK on Linux, and
nothing worked if either was missing or broken.

1.0.1 takes the browser out. Windows and widgets are drawn by `egui` on the
`glow` renderer. There is no HTML, no CSS, no JavaScript and no WebView
anywhere in the pipeline, and `rakc` no longer depends on `wry` or `tao`.
Building `--features gui` on Linux no longer needs `libwebkit2gtk`.

### The API changed

GUI programs written against the old builtins need editing.

| Before | Now |
|---|---|
| `gui_open(title, html, w, h)` | `gui_window(title, w, h)` + `gui_add(...)` |
| `gui_update(id, html)` | `gui_set_text(widget, text)` |
| `gui_title(id, title)` | set the title at creation; no runtime equivalent |
| — | `gui_set_visible(widget, visible)` |
| — | `gui_get_text(widget)` |
| `rak_call(name, ...)` from page script | `gui_on_click(widget, name)` |

```rak
fn on_widget(name, message) {
  print f"{name} says: {message}"
  return nil
}

gui_callback("on_widget", on_widget)

let win = gui_window("Rak GUI Demo", 420, 240)
let button = gui_add(win, "button", "Click me", 20, 20, 140, 34)
let label = gui_add(win, "label", "waiting", 20, 70, 240, 24)

gui_on_click(button, "on_widget")
gui_set_text(label, "clicked at least once")

gui_wait()
```

Widget kinds are `label`, `button`, `input`, `checkbox`, `select` and
`textarea`. A `select` takes its options as one comma-separated string.
`gui_get_text` returns the text an input holds, `"true"`/`"false"` for a
checkbox, and the caption otherwise.

### The designer emits native code

The IDE's GUI Designer previously generated an HTML blob for `gui_open`. It now
generates the native calls above, so a designed window is the same code you
would write by hand. It no longer offers an image widget, which the native
backend cannot render.

### Three bugs fixed in the GUI driver

All three predate this rewrite and were reachable whenever the `gui` feature was
enabled:

- `max_depth` was never handed to the interpreter, so `rakc run --max-depth` was
  silently ignored.
- An error raised inside `fn main` collapsed to a bare exit code, so the message
  never reached stderr.
- Every `rakc run` started a window even when the script never opened one. The
  event loop now waits for the script to actually ask for a window, which is
  what was pushing the long examples and the benchmarks past their timeouts
  under `--features gui`.