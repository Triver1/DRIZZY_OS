 
from ignis import widgets
from ignis.services.applications import ApplicationsService

from rapidfuzz import process
from sharedwidgets import PopupWindow
import os
import threading
import subprocess
import time
from google import genai
from google.genai import types

applications = ApplicationsService.get_default()




class AppsList(widgets.Box):

    def app_launch(self, app):
        app.launch()
        self.close_callback()

    def __init__(self, close_callback):
        self.apps = applications.apps
        super().__init__(
            vertical=True,
                         child=[]
        )
        self.filter("")
        self.close_callback = close_callback


    def app_widget(self, app): 
        return widgets.Button(
            css_classes=["launcherapp"],
            child=widgets.Box(spacing=10, child=[
                widgets.Icon(image=app.icon, pixel_size=20),
                widgets.Label(label=app.name)
            ]),
            on_click=lambda x: app_launch(app)
        )
         
    def filter(self, filter_string):
        if not filter_string:
            apps = []
        else:
            matches = process.extract(filter_string, [app.name for app in self.apps], limit=10, score_cutoff=40)
            app_dict = {app.name: app for app in self.apps}
            apps = [app_dict[match[0]] for match in matches]
        
        self.child = [self.app_widget(app) for app in apps]
    
    def launch_first(self, filter_string):
        apps = []
        if filter_string:
            matches = process.extract(filter_string, [app.name for app in self.apps], limit=1, score_cutoff=40)
            app_dict = {app.name: app for app in self.apps}
            apps = [app_dict[match[0]] for match in matches]
        
        if len(apps) > 0:
            apps[0].launch()


class UtilsList(widgets.Box):
    def __init__(self, commands: list[tuple[str, str, callable]], on_execute, close_callback):
        self.commands = commands
        self.on_execute = on_execute
        super().__init__(
            vertical=True,
            child=[]
        )
        self.close_callback = close_callback
        self.filter("")

    def _command_widget(self, name: str, description: str, callback):
        return widgets.Button(
            css_classes=["launcherapp"],
            child=widgets.Box(spacing=10, child=[
                widgets.Label(label=f"/{name}"),
                widgets.Label(label=description)
            ]),
            on_click=lambda x: self._execute(callback)
        )

    def _execute(self, callback):
        try:
            callback()
        finally:
            self.close_callback()

    def filter(self, filter_string: str):
        if filter_string:
            query = (filter_string or "").lower()
            names_lower = [name.lower() for (name, _desc, _cb) in self.commands]
            matches = process.extract(query, names_lower, limit=10, score_cutoff=40)
            lower_to_cmd = {name.lower(): (name, desc, cb) for (name, desc, cb) in self.commands}
            filtered = [lower_to_cmd[m[0]] for m in matches]
        else:
            filtered = self.commands
        self.child = [self._command_widget(name, desc, cb) for (name, desc, cb) in filtered]

    def run_first(self, filter_string: str):
        candidates = []
        if filter_string:
            query = (filter_string or "").lower()
            names_lower = [name.lower() for (name, _desc, _cb) in self.commands]
            matches = process.extract(query, names_lower, limit=1, score_cutoff=40)
            lower_to_cmd = {name.lower(): (name, desc, cb) for (name, desc, cb) in self.commands}
            candidates = [lower_to_cmd[m[0]] for m in matches]
        else:
            candidates = self.commands[:1]
        if candidates:
            _name, _desc, cb = candidates[0]
            self._execute(cb)


class AIWindow(PopupWindow):
    def __init__(self):
        self._messages = widgets.Box(
            vertical=True,
            spacing=8,
            child=[],
            css_classes=["ai-messages"],
            halign="center",
        )
        self._scroll = widgets.Scroll(
            child=self._messages,
            min_content_height=360,
            max_content_height=560,
            css_classes=["ai-scroll"],
        )
        self._input = widgets.Entry(
            placeholder_text="✨ Ask something...",
            on_accept=lambda entry: self._on_submit(entry.text),
            css_classes=["launcherinput"],
            hexpand=True,
        )
        content = widgets.Box(
            vertical=True,
            valign="start",
            halign="center",
            spacing=10,
            css_classes=["launcher", "ai-chat"],
            child=[
                self._scroll,
                self._input,
            ],
        )
        super().__init__(
            child=content,
            namespace="AIWindow",
            monitor=0,
            anchor=["top", "right", "bottom", "left"],
            layer="top",
            kb_mode="exclusive",
            popup=True,
            background_color="rgba(0, 0, 0, 0.1)",
            content_halign="center",
            content_valign="start",
            css_classes=["launcherwindow"],
        )
        self.connect("notify::visible", self.__on_open)
        self._client = None
        # Conversation history as Content objects to allow multimodal turns
        self._history: list[types.Content] = []

    def __on_open(self, *args):
        if self.visible:
            self._input.grab_focus()
        else:
            # Clear chat when closed
            self._messages.child = []
            self._history = []

    def _append_message(self, role: str, text: str):
        if role == "assistant":
            content_box = widgets.Box(vertical=True, spacing=2, child=[])
            bubble_child = [content_box]
        else:
            label = widgets.Label(label=text, wrap=True, max_width_chars=80)
            content_box = label
            bubble_child = [label]
        bubble = widgets.Box(
            css_classes=["chat-bubble", f"role-{role}"],
            child=bubble_child,
            hexpand=False,
        )
        row = widgets.Box(
            hexpand=True,
            halign="end" if role == "user" else "start",
            child=[bubble],
        )
        self._messages.append(row)
        return content_box

    def _on_submit(self, text: str):
        if not text:
            return
        self._append_message("user", text)
        self._history.append(
            types.Content(role="user", parts=[types.Part.from_text(text=text)])
        )
        assistant_container = self._append_message("assistant", "")
        self._input.text = ""
        self._stream_response(assistant_container)

    def _ensure_client(self):
        if self._client is None:
            api_key = os.environ.get("GEMINI_API_KEY")
            self._client = genai.Client(api_key=api_key)

    def _build_contents(self):
        # History already stored as Content objects
        return list(self._history)

    def _stream_response(self, label_widget):
        def worker():
            try:
                self._ensure_client()
                model = "gemini-2.0-flash"
                contents = self._build_contents() or [
                    types.Content(role="user", parts=[types.Part.from_text(text="Hello")])
                ]
                tools = [types.Tool(googleSearch=types.GoogleSearch())]
                config = types.GenerateContentConfig(
                    tools=tools,
                )
                accumulated = ""
                for chunk in self._client.models.generate_content_stream(
                    model=model,
                    contents=contents,
                    config=config,
                ):
                    text = getattr(chunk, "text", "") or ""
                    if text:
                        accumulated += text
                        # Render markdown progressively for assistant
                        try:
                            if hasattr(label_widget, "label"):
                                label_widget.label = accumulated
                            else:
                                label_widget.child = [self._render_markdown(accumulated)]
                        except Exception:
                            pass
                # finalize history with assistant output
                if accumulated:
                    self._history.append(
                        types.Content(role="model", parts=[types.Part.from_text(text=accumulated)])
                    )
            except Exception as e:
                try:
                    if hasattr(label_widget, "label"):
                        label_widget.label = f"Error: {e}"
                    else:
                        label_widget.child = [widgets.Label(label=f"Error: {e}", wrap=True, max_width_chars=80)]
                except Exception:
                    pass

        threading.Thread(target=worker, daemon=True).start()

    def open(self, initial_prompt: str | None = None):
        self.visible = True
        if initial_prompt:
            self._input.text = initial_prompt
            self._on_submit(initial_prompt)

    def open_with_image(self, prompt_text: str, image_bytes: bytes, mime_type: str = "image/png"):
        self.visible = True
        # Visual cue
        self._append_message("user", prompt_text or "Explain this screenshot")
        # Append multimodal user turn
        parts = [
            types.Part.from_text(text=prompt_text or "Explain this screenshot"),
            types.Part.from_bytes(data=image_bytes, mime_type=mime_type),
        ]
        self._history.append(types.Content(role="user", parts=parts))
        assistant_container = self._append_message("assistant", "")
        self._stream_response(assistant_container)

    def _render_markdown(self, text: str):
        lines = (text or "").splitlines()
        children = []
        in_code = False
        code_buffer: list[str] = []

        def flush_code():
            nonlocal code_buffer
            if code_buffer:
                code_text = "\n".join(code_buffer)
                children.append(widgets.Label(label=code_text, wrap=True, max_width_chars=80, css_classes=["md-code"]))
                code_buffer = []

        for line in lines:
            if line.strip().startswith("```"):
                if in_code:
                    flush_code()
                    in_code = False
                else:
                    in_code = True
                continue
            if in_code:
                code_buffer.append(line)
                continue

            stripped = line.strip()
            if not stripped:
                children.append(widgets.Box(child=[]))
                continue
            if stripped.startswith("### "):
                children.append(widgets.Label(label=stripped[4:], css_classes=["md-h3"], wrap=True, max_width_chars=80))
                continue
            if stripped.startswith("## "):
                children.append(widgets.Label(label=stripped[3:], css_classes=["md-h2"], wrap=True, max_width_chars=80))
                continue
            if stripped.startswith("# "):
                children.append(widgets.Label(label=stripped[2:], css_classes=["md-h1"], wrap=True, max_width_chars=80))
                continue
            if stripped.startswith("- ") or stripped.startswith("* "):
                bullet = widgets.Label(label="• ")
                content = self._render_inline(stripped[2:])
                children.append(widgets.Box(child=[bullet, content]))
                continue
            # paragraph
            children.append(self._render_inline(stripped))

        flush_code()
        return widgets.Box(vertical=True, spacing=2, child=children)

    def _render_inline(self, text: str):
        parts = []
        i = 0
        n = len(text)
        while i < n:
            if text.startswith("**", i):
                j = text.find("**", i + 2)
                if j != -1:
                    seg = text[i + 2:j]
                    parts.append(widgets.Label(label=seg, css_classes=["md-bold"], wrap=True, max_width_chars=80))
                    i = j + 2
                    continue
            if text.startswith("*", i):
                j = text.find("*", i + 1)
                if j != -1:
                    seg = text[i + 1:j]
                    parts.append(widgets.Label(label=seg, css_classes=["md-italic"], wrap=True, max_width_chars=80))
                    i = j + 1
                    continue
            # plain char sequence until next marker
            next_i = n
            for marker in ["**", "*"]:
                k = text.find(marker, i)
                if k != -1:
                    next_i = min(next_i, k)
            seg = text[i:next_i]
            if seg:
                parts.append(widgets.Label(label=seg, wrap=True, max_width_chars=80))
            i = next_i
        if len(parts) == 1:
            return parts[0]
        return widgets.Box(child=parts)


class TranslateWindow(PopupWindow):
    def __init__(self):
        self._source = widgets.Entry(
            placeholder_text="Type text to translate...",
            on_accept=lambda entry: self._translate(entry.text),
            css_classes=["launcherinput"],
            hexpand=True,
        )
        self._result = widgets.Label(label="")
        search_box = widgets.Box(
            css_classes=["launcher-search-box"],
            child=[
                widgets.Icon(
                    icon_name="system-search-symbolic",
                    pixel_size=24,
                    style="margin-right: 0.5rem;",
                ),
                self._source,
            ],
        )
        content = widgets.Box(
            vertical=True,
            valign="start",
            halign="center",
            spacing=10,
            css_classes=["launcher"],
            child=[
                search_box,
                widgets.Separator(),
                widgets.Box(child=[widgets.Label(label="Result:"), self._result]),
            ],
        )
        super().__init__(
            child=content,
            namespace="TranslateWindow",
            monitor=0,
            anchor=["top", "right", "bottom", "left"],
            layer="top",
            kb_mode="exclusive",
            popup=True,
            background_color="rgba(0, 0, 0, 0.1)",
            content_halign="center",
            content_valign="start",
            css_classes=["launcherwindow"],
        )
        self.connect("notify::visible", self.__on_open)

    def __on_open(self, *args):
        if self.visible:
            self._source.grab_focus()

    def _translate(self, text: str):
        # Placeholder translation
        if text:
            self._result.label = f"[Translated] {text}"
        else:
            self._result.label = ""

    def open(self, initial_text: str | None = None):
        self.visible = True
        if initial_text:
            self._source.text = initial_text
            self._translate(initial_text)


class Launcher(PopupWindow):
    def __init__(self):
        self.apps_list = AppsList(lambda x: self.set_visible(False))
        # Utils commands
        self.ai_window = AIWindow()
        self.translate_window = TranslateWindow()
        self.commands = [
            (
                "AI",
                "Open AI chat",
                lambda: self._open_ai(None),
            ),
            (
                "TS",
                "Open Translate",
                lambda: self._open_translate(None),
            ),
            (
                "EXPLAIN",
                "Explain clipboard contents",
                lambda: self._open_ai_explain(None),
            ),
            (
                "EXPLAIN_SCREEN",
                "Explain what is on the screen",
                lambda: self._explain_screen(None),
            ),
        ]
        self.utils_list = UtilsList(
            self.commands,
            on_execute=lambda: None,
            close_callback=lambda: setattr(self, "visible", False),
        )

        self.input = widgets.Entry(
            placeholder_text="Filter apps or type / for utils",
            on_accept=lambda x: self.launch(x.text),
            on_change=lambda x: self._on_input_change(x.text),
            css_classes=["launcherinput"],
            hexpand=True
        )

        search_box = widgets.Box(
            css_classes=["launcher-search-box"],
            child=[
                widgets.Icon(
                    icon_name="system-search-symbolic",
                    pixel_size=24,
                    style="margin-right: 0.5rem;",
                ),
                self.input,
            ],
        )

        # Results area switches between apps and utils (no animation)
        self.results_stack = widgets.Stack()
        self.results_stack.add_named(self.apps_list, "apps")
        self.results_stack.add_named(self.utils_list, "utils")

        main_box = widgets.Box(
            vertical=True,
            valign="start",     # Expand downward
            halign="center",    # Center horizontally
            spacing=10,
            css_classes=["launcher"],
            child=[
                search_box,
                self.results_stack
            ]
        )

        super().__init__(
            child=main_box,
            namespace="launcher",
            monitor=0,
            anchor=["top", "right", "bottom", "left"],
            layer="top",
            kb_mode="exclusive",
            popup=True,
            background_color="rgba(0, 0, 0, 0.1)",
            css_classes=["launcherwindow"],
            content_anchor=["top"],
            content_halign="center",
            content_valign="start",
        )

        self.connect("notify::visible", self.__on_open)

    def launch(self, input):
        if isinstance(input, str) and input.startswith("/"):
            # Close launcher immediately for utils commands (e.g., /EXPLAIN_SCREEN)
            self.visible = False
            self._handle_command(input)
            return
        self.apps_list.launch_first(input)
        self.visible = False

    def __on_open(self, *args) -> None:
        self.input.text = ""
        self.input.grab_focus()

    def _on_input_change(self, text: str):
        if isinstance(text, str) and text.startswith("/"):
            query = text[1:].strip()
            self.results_stack.set_visible_child_name("utils")
            self.utils_list.filter(query)
        else:
            self.results_stack.set_visible_child_name("apps")
            self.apps_list.filter(text)

    def _handle_command(self, raw: str):
        cmdline = (raw or "/").strip()
        if not cmdline.startswith("/"):
            return
        body = cmdline[1:].strip()
        if not body:
            # no command, just show utils list
            self.results_stack.set_visible_child_name("utils")
            return
        parts = body.split(None, 1)
        cmd = parts[0].upper()
        arg = parts[1] if len(parts) > 1 else None
        if cmd == "AI":
            self._open_ai(arg)
        elif cmd == "TS":
            self._open_translate(arg)
        elif cmd == "EXPLAIN":
            self._open_ai_explain(arg)
        elif cmd == "EXPLAIN_SCREEN":
            # Ensure launcher is hidden before taking screenshot
            self.visible = False
            self._explain_screen(arg)
        else:
            # Fallback: run best match from utils list
            self.utils_list.run_first(body)

    def _open_ai(self, initial_prompt: str | None):
        try:
            self.ai_window.open(initial_prompt)
        finally:
            self.visible = False

    def _open_translate(self, initial_text: str | None):
        try:
            self.translate_window.open(initial_text)
        finally:
            self.visible = False

    def _open_ai_explain(self, extra_context: str | None):
        try:
            text = self._read_selection_text() or self._read_clipboard_text()
            if text:
                prompt = f"Explain the following text/code in simple terms, with examples if relevant.\n\n{text}"
                if extra_context:
                    prompt += f"\n\nAdditional context: {extra_context}"
            else:
                prompt = "Explain: (clipboard was empty)"
            self.ai_window.open(prompt)
        finally:
            self.visible = False

    def _read_selection_text(self) -> str:
        # Try Wayland primary selection
        try:
            result = subprocess.run(["wl-paste", "-p", "-n"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        # Try X11 primary via xclip
        try:
            result = subprocess.run(["xclip", "-o", "-selection", "primary"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        # Try X11 primary via xsel
        try:
            result = subprocess.run(["xsel", "--primary", "--output"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        return ""

    def _explain_screen(self, extra: str | None):
        try:
            # Hide launcher while user selects the area
            self.visible = False
            # Ensure the AI window is hidden before taking the screenshot
            try:
                self.ai_window.visible = False
            except Exception:
                pass
            # Snapshot current clipboard image so we don't pick an old one
            initial_img, _initial_mime = self._read_clipboard_image()

            prompt = "Explain what is on this screenshot. Be concise, then list key elements."
            if extra:
                prompt += f"\nAdditional context: {extra}"

            def worker():
                # Give compositor a moment to hide windows before capturing
                time.sleep(0.2)
                try:
                    subprocess.run(["niri", "msg", "action", "screenshot"], check=False)
                except Exception:
                    pass

                deadline = time.monotonic() + 12.0
                while time.monotonic() < deadline:
                    img, mime = self._read_clipboard_image()
                    # Accept only if we have a new image different from the snapshot
                    if img and (initial_img is None or img != initial_img):
                        try:
                            self.ai_window.open_with_image(prompt, img, mime or "image/png")
                        except Exception:
                            self._open_ai("Explain this screenshot: (failed to open AI window)")
                        return
                    time.sleep(0.25)
                # Timeout
                self._open_ai("Explain this screenshot: (no image found in clipboard)")

            threading.Thread(target=worker, daemon=True).start()
        finally:
            # keep hidden while selection happens
            self.visible = False

    def _read_clipboard_image(self) -> tuple[bytes | None, str | None]:
        # Try Wayland clipboard
        try:
            proc = subprocess.run(["wl-paste", "-t", "image/png"], capture_output=True, timeout=0.8)
            if proc.returncode == 0 and proc.stdout:
                data = proc.stdout
                return data, "image/png"
        except Exception:
            pass
        # Try JPEG on Wayland
        try:
            proc = subprocess.run(["wl-paste", "-t", "image/jpeg"], capture_output=True, timeout=0.8)
            if proc.returncode == 0 and proc.stdout:
                data = proc.stdout
                return data, "image/jpeg"
        except Exception:
            pass
        # Try X11 clipboard via xclip
        for mime in ["image/png", "image/jpeg"]:
            try:
                proc = subprocess.run(["xclip", "-selection", "clipboard", "-o", "-t", mime], capture_output=True, timeout=0.8)
                if proc.returncode == 0 and proc.stdout:
                    return proc.stdout, mime
            except Exception:
                pass
        return None, None

    def _read_clipboard_text(self) -> str:
        # Try Wayland first
        try:
            result = subprocess.run(["wl-paste", "-n"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        # Try xclip
        try:
            result = subprocess.run(["xclip", "-o", "-selection", "clipboard"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        # Try xsel
        try:
            result = subprocess.run(["xsel", "--clipboard", "--output"], capture_output=True, text=True, timeout=0.5)
            text = (result.stdout or "").strip()
            if text:
                return text
        except Exception:
            pass
        return ""

