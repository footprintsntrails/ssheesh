#!/usr/bin/env python3
"""A made-up freedesktop notification server for `tests/freedesktop.rs`.

It takes the name org.freedesktop.Notifications on the session bus, answers Notify with a
number, and a moment later "clicks" what it was shown: ActivationToken, then ActionInvoked
with the "default" action, as GNOME and KDE do. Each Notify call is printed to stdout as one
JSON line, for the test to read.

    dbus-run-session -- sh -c 'python3 tests/fake_notifications.py & sleep 1; \
        cargo test -p sshee-desktop --test freedesktop -- --ignored'

Needs python3 with dbus-python and PyGObject (python3-dbus, python3-gi).
"""
import json
import sys

import dbus
import dbus.service
from dbus.mainloop.glib import DBusGMainLoop
from gi.repository import GLib

NAME = "org.freedesktop.Notifications"
PATH = "/org/freedesktop/Notifications"


class Server(dbus.service.Object):
    def __init__(self, bus):
        super().__init__(dbus.service.BusName(NAME, bus), PATH)
        self.next = 0

    @dbus.service.method(NAME, in_signature="", out_signature="as")
    def GetCapabilities(self):
        return ["actions", "body", "body-markup", "persistence"]

    @dbus.service.method(NAME, in_signature="susssasa{sv}i", out_signature="u")
    def Notify(self, app, replaces, icon, title, body, actions, hints, timeout):
        if replaces:
            number = int(replaces)
        else:
            self.next += 1
            number = self.next
        print(json.dumps({"app": str(app), "id": number, "replaces": int(replaces), "title": str(title), "body": str(body), "actions": [str(a) for a in actions]}), flush=True)
        GLib.timeout_add(300, self.click, number)
        return number

    @dbus.service.method(NAME, in_signature="u", out_signature="")
    def CloseNotification(self, number):
        print(json.dumps({"closed": int(number)}), flush=True)
        self.NotificationClosed(number, 3)

    @dbus.service.method(NAME, in_signature="", out_signature="ssss")
    def GetServerInformation(self):
        return ("fake", "sshee", "1", "1.2")

    @dbus.service.signal(NAME, signature="uu")
    def NotificationClosed(self, number, reason):
        pass

    @dbus.service.signal(NAME, signature="us")
    def ActionInvoked(self, number, action):
        pass

    @dbus.service.signal(NAME, signature="us")
    def ActivationToken(self, number, token):
        pass

    def click(self, number):
        self.ActivationToken(number, "fake-activation-token")
        self.ActionInvoked(number, "default")
        return False


def main():
    DBusGMainLoop(set_as_default=True)
    Server(dbus.SessionBus())
    print(json.dumps({"ready": True}), flush=True)
    GLib.MainLoop().run()


if __name__ == "__main__":
    sys.exit(main())
