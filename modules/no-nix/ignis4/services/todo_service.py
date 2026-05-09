from gi.repository import GLib
import os

TODO_FILE = os.path.expanduser("~/todos.md")

# Singleton instance
_instance = None


def get_todo_service():
    global _instance
    if _instance is None:
        _instance = TodoService()
    return _instance


class TodoService:
    def __init__(self):
        self.timer_active = False
        self.timer_seconds = 25 * 60
        self.remaining = 25 * 60
        self.current_todo_index = 0
        self._timer_id = None
        self._listeners = []

    def add_listener(self, callback):
        self._listeners.append(callback)

    def _notify(self):
        for cb in self._listeners:
            try:
                cb()
            except Exception:
                pass

    # -- File I/O --

    @staticmethod
    def read_todos():
        if not os.path.exists(TODO_FILE):
            return []
        todos = []
        with open(TODO_FILE, "r") as f:
            for line in f:
                line = line.strip()
                if line.startswith("- [x] "):
                    todos.append((line[6:], True))
                elif line.startswith("- [ ] "):
                    todos.append((line[6:], False))
                elif line.startswith("- "):
                    todos.append((line[2:], False))
        return todos

    @staticmethod
    def write_todos(todos):
        with open(TODO_FILE, "w") as f:
            f.write("# Todos\n\n")
            for text, done in todos:
                mark = "x" if done else " "
                f.write(f"- [{mark}] {text}\n")

    def add_todo(self, text):
        todos = self.read_todos()
        todos.append((text, False))
        self.write_todos(todos)
        self._notify()

    def toggle_done(self, index):
        todos = self.read_todos()
        if 0 <= index < len(todos):
            text, done = todos[index]
            todos[index] = (text, not done)
            self.write_todos(todos)
            self._notify()

    def remove_todo(self, index):
        todos = self.read_todos()
        if 0 <= index < len(todos):
            todos.pop(index)
            self.write_todos(todos)
            self._notify()

    # -- Timer --

    def get_active_todo_text(self):
        todos = self.read_todos()
        incomplete = [t for t, d in todos if not d]
        if incomplete and self.current_todo_index < len(incomplete):
            return incomplete[self.current_todo_index]
        return None

    def get_timer_display(self):
        mins = self.remaining // 60
        secs = self.remaining % 60
        return f"{mins:02d}:{secs:02d}"

    def set_duration(self, seconds):
        if self.timer_active:
            return
        self.timer_seconds = seconds
        self.remaining = seconds
        self._notify()

    def start_timer(self):
        todos = self.read_todos()
        incomplete = [t for t, d in todos if not d]
        if not incomplete:
            return
        self.timer_active = True
        self.remaining = self.timer_seconds
        if self.current_todo_index >= len(incomplete):
            self.current_todo_index = 0
        self._notify()
        self._timer_id = GLib.timeout_add(1000, self._tick)

    def stop_timer(self):
        self.timer_active = False
        if self._timer_id:
            GLib.source_remove(self._timer_id)
            self._timer_id = None
        self._notify()

    def toggle_timer(self):
        if self.timer_active:
            self.stop_timer()
        else:
            self.start_timer()

    def _tick(self):
        if not self.timer_active:
            return False
        self.remaining -= 1
        if self.remaining <= 0:
            self._cycle_todo()
            self.remaining = self.timer_seconds
        self._notify()
        return True

    def _cycle_todo(self):
        todos = self.read_todos()
        incomplete = [t for t, d in todos if not d]
        if not incomplete:
            self.stop_timer()
            return
        self.current_todo_index = (self.current_todo_index + 1) % len(incomplete)
