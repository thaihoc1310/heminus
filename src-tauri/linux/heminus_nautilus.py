"""Nautilus extension that adds "Open in Heminus" to folder context menus.

Installed to /usr/share/nautilus-python/extensions/ by the Heminus deb.
Requires the python3-nautilus package; Nautilus loads it on next start.
"""

import subprocess
from urllib.parse import unquote, urlparse

from gi.repository import GObject, Nautilus

HEMINUS_BINARY = "/usr/bin/heminus-app"


def _local_folder_path(item):
    if item.get_uri_scheme() != "file" or not item.is_directory():
        return None
    return unquote(urlparse(item.get_uri()).path)


class HeminusOpenTerminal(GObject.GObject, Nautilus.MenuProvider):
    def _launch(self, _menu, path):
        subprocess.Popen(
            [HEMINUS_BINARY, "--new-terminal", "--cwd", path],
            start_new_session=True,
        )

    def _menu_item(self, name, path):
        item = Nautilus.MenuItem(
            name=name,
            label="Open in Heminus",
            tip="Open a Heminus terminal in this folder",
        )
        item.connect("activate", self._launch, path)
        return item

    # nautilus-python < 4.0 passes (window, files); 4.0+ passes (files).
    def get_file_items(self, *args):
        files = args[-1]
        if len(files) != 1:
            return []
        path = _local_folder_path(files[0])
        if path is None:
            return []
        return [self._menu_item("HeminusOpenTerminal::open_selected", path)]

    def get_background_items(self, *args):
        path = _local_folder_path(args[-1])
        if path is None:
            return []
        return [self._menu_item("HeminusOpenTerminal::open_background", path)]
