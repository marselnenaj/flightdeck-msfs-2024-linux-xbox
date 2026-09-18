#!/usr/bin/env python3
# SPDX-License-Identifier: MIT
"""Optional graphical source installer; uses an already available local toolkit."""
from __future__ import annotations

import html
import os
import queue
import shutil
import subprocess
import sys
import threading

COPY = {
    "en": {
        "title": "Install Flightdeck",
        "question": "Install Flightdeck for your user and open setup?\n\nYour game, account and save files are preserved. No game is downloaded.",
        "remove": "Remove the managed Flightdeck launcher?\n\nSettings, runtimes, accounts and saves are preserved.",
        "install": "Install",
        "uninstall": "Uninstall",
        "cancel": "Cancel",
        "close": "Close",
        "working": "Preparing the launcher …",
        "missing": "A graphical installer needs Zenity, kdialog or Python Tk, and a desktop session. Run ./install.sh in a terminal instead.",
        "launch_failed": "Installation completed, but the launcher did not start. Open Flightdeck from the application menu.",
    },
    "de": {
        "title": "Flightdeck installieren",
        "question": "Flightdeck für deinen Benutzer installieren und die Einrichtung öffnen?\n\nSpiel, Konto und Spielstände bleiben erhalten. Es wird kein Spiel heruntergeladen.",
        "remove": "Den verwalteten Flightdeck-Launcher entfernen?\n\nEinstellungen, Runtimes, Konten und Spielstände bleiben erhalten.",
        "install": "Installieren",
        "uninstall": "Deinstallieren",
        "cancel": "Abbrechen",
        "close": "Schließen",
        "working": "Launcher wird vorbereitet …",
        "missing": "Der grafische Installer benötigt Zenity, kdialog oder Python Tk und eine Desktopsitzung. Alternativ ./install.sh im Terminal starten.",
        "launch_failed": "Installation abgeschlossen, aber der Launcher konnte nicht starten. Öffne Flightdeck über das Anwendungsmenü.",
    },
}


def command_dialog(kind, text, language, *, remove=False):
    """Return argument vectors, never shell commands or markup from paths."""
    words = COPY[language]
    zenity = shutil.which("zenity")
    if zenity:
        command = [zenity, "--" + {"question": "question", "error": "error", "info": "info"}[kind],
                   "--no-markup", "--title=" + words["title"], "--text=" + text]
        if kind == "question":
            command += ["--ok-label=" + words["uninstall" if remove else "install"],
                        "--cancel-label=" + words["cancel"]]
        else:
            command += ["--ok-label=" + words["close"]]
        return command
    kdialog = shutil.which("kdialog")
    if kdialog:
        command = [kdialog, "--title", words["title"],
                   {"question": "--yesno", "error": "--error", "info": "--msgbox"}[kind], html.escape(text)]
        if kind == "question":
            command += ["--yes-label", words["uninstall" if remove else "install"], "--no-label", words["cancel"]]
        return command
    return None


def launch(command, explicit_language):
    if command is None:
        return
    arguments = [sys.executable, command]
    if explicit_language:
        arguments += ["--language", explicit_language]
    # The installed wrapper selects the CLI's detached desktop mode. Do not
    # attach a terminal or inherit the installer's progress pipe.
    subprocess.Popen(arguments, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                     stderr=subprocess.DEVNULL, close_fds=True, start_new_session=True)


def external_dialog(operation, explain, language, explicit_language, *, uninstalling=False):
    words = COPY[language]
    question = command_dialog("question", words["remove" if uninstalling else "question"], language, remove=uninstalling)
    if subprocess.run(question, check=False).returncode:
        return 0
    progress = None
    zenity = shutil.which("zenity")
    if zenity:
        progress = subprocess.Popen([zenity, "--progress", "--pulsate", "--auto-close", "--no-cancel",
                                     "--title=" + words["title"], "--text=" + words["working"]],
                                    stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                    text=True)
    try:
        message, launcher = operation()
        failure = False
    except Exception as error:
        message, launcher, failure = explain(error), None, True
    finally:
        if progress:
            try:
                progress.communicate("100\n", timeout=5)
            except (BrokenPipeError, subprocess.TimeoutExpired):
                progress.terminate()
                try:
                    progress.communicate(timeout=2)
                except subprocess.TimeoutExpired:
                    progress.kill()
                    progress.communicate()
    if launcher:
        try:
            launch(launcher, explicit_language)
        except OSError:
            message, failure = words["launch_failed"], True
        else:
            return 0
    subprocess.run(command_dialog("error" if failure else "info", message, language), check=False)
    return 1 if failure else 0


def tkinter_dialog(operation, explain, language, explicit_language, *, uninstalling=False):
    import tkinter as tk
    from tkinter import ttk
    words = COPY[language]
    window = tk.Tk()
    window.title(words["title"])
    window.resizable(False, False)
    frame = ttk.Frame(window, padding=24)
    frame.pack(fill="both", expand=True)
    status = tk.StringVar(value=words["remove" if uninstalling else "question"])
    ttk.Label(frame, textvariable=status, wraplength=480, justify="left").pack(fill="x", pady=(0, 18))
    progress = ttk.Progressbar(frame, mode="indeterminate")
    controls = ttk.Frame(frame)
    controls.pack(fill="x")
    messages = queue.Queue()
    active = False
    result = 0

    def close():
        if not active:
            window.destroy()

    def finish():
        nonlocal active, result
        try:
            message, launcher, failed = messages.get_nowait()
        except queue.Empty:
            window.after(75, finish)
            return
        active = False
        progress.stop()
        progress.pack_forget()
        result = 1 if failed else 0
        if launcher:
            try:
                launch(launcher, explicit_language)
            except OSError:
                status.set(words["launch_failed"])
                result = 1
            else:
                close()
                return
        else:
            status.set(message)
        accept.pack_forget()
        cancel.configure(text=words["close"], state="normal")

    def begin():
        nonlocal active
        active = True
        accept.configure(state="disabled")
        cancel.configure(state="disabled")
        status.set(words["working"])
        progress.pack(fill="x", pady=12)
        progress.start()
        def work():
            try:
                message, launcher = operation()
                messages.put((message, launcher, False))
            except Exception as error:
                messages.put((explain(error), None, True))
        # Do not abandon an in-progress atomic installation if the window closes.
        threading.Thread(target=work, daemon=False).start()
        window.after(75, finish)

    cancel = ttk.Button(controls, text=words["cancel"], command=close)
    cancel.pack(side="right")
    accept = ttk.Button(controls, text=words["uninstall" if uninstalling else "install"], command=begin)
    accept.pack(side="right", padx=8)
    window.protocol("WM_DELETE_WINDOW", close)
    window.mainloop()
    return result


def run(operation, explain, language, explicit_language, *, uninstalling=False):
    if os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"):
        if command_dialog("question", "", language):
            return external_dialog(operation, explain, language, explicit_language, uninstalling=uninstalling)
        try:
            return tkinter_dialog(operation, explain, language, explicit_language, uninstalling=uninstalling)
        except ImportError:
            pass
        except Exception as error:
            # Tk can be importable without an accessible display. Report that
            # before any operation, rather than silently attempting CLI install.
            print(explain(error), file=sys.stderr)
            return 1
    print(COPY[language]["missing"], file=sys.stderr)
    return 1
