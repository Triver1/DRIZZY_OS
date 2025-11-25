import subprocess
from typing import List

from gi.repository import GLib

try:
    from ignis.base_service import BaseService
    from ignis import utils
except Exception:  # Fallback types if ignis isn't fully available at import time
    BaseService = object  # type: ignore
    utils = None  # type: ignore


class MangoService(BaseService):
    _default = None

    @classmethod
    def get_default(cls):
        if cls._default is None:
            cls._default = cls()
        return cls._default

    def __init__(self):
        super().__init__()
        self.__poll_interval_ms = 500
        self.__last_tags: List[int] = []
        # Periodic polling (kept internal; components can call get_tags or use their own Poll)
        GLib.timeout_add(self.__poll_interval_ms, self.__tick)

    def __tick(self):
        try:
            self.__last_tags = self.__fetch_tags()
        except Exception:
            # Keep last known state on error
            pass
        return True  # keep polling

    def get_tags(self) -> List[int]:
        """Return current workspace tag numbers.

        Minimal implementation using the external `mangowc` client if available.
        Fallbacks to an empty list on error.
        """
        try:
            return self.__fetch_tags()
        except Exception:
            return self.__last_tags or []

    def __fetch_tags(self) -> List[int]:
        """Call `mmsg -g` and parse active workspace tag numbers.

        Expected lines example per monitor:
            eDP-1 tag 1 1 4 1
            eDP-1 tag 2 0 2 0
            ...
            eDP-1 tags 3 1 0
            eDP-1 tags 000000011 000000001 000000000

        We primarily use the `tag <n> <sel> <clients> <urgent>` lines and return
        tag numbers where <sel> == 1. If none are selected, we fallback to tags
        with clients > 0. If still none, return an empty list.
        """
        out = subprocess.check_output(["mmsg", "-g"], stderr=subprocess.DEVNULL, text=True, timeout=0.4)

        selected: List[int] = []
        occupied: List[int] = []

        for raw_line in out.splitlines():
            line = raw_line.strip()
            if not line:
                continue
            parts = line.split()
            # Expect: <monitor> tag <n> <sel> <clients> <urgent>
            if len(parts) >= 6 and parts[1] == "tag":
                try:
                    tag_num = int(parts[2])
                    sel_flag = int(parts[3])
                    clients = int(parts[4])
                except Exception:
                    continue
                if sel_flag == 1:
                    selected.append(tag_num)
                if clients > 0:
                    occupied.append(tag_num)

        if selected:
            return sorted(selected)
        if occupied:
            return sorted(set(occupied))
        return []


