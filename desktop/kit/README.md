# Measuring kit

Scripts that measure SSHeesh on your own computer, for comparing the 1.0 prototype with 0.10.

| File | What it does |
|---|---|
| `measure.ps1` | Windows: the measurements. Run it inside a terminal of the app being measured ("This computer"). |
| `measure.sh` | macOS and Linux: the same. |
| `run-prototype.ps1` | Windows: starts the 1.0 prototype with its meter on. |
| `run-prototype.sh` | macOS and Linux: the same. |

What is measured: memory before and after 10,100 lines, how fast a large file prints, Cyrillic
against Latin text, and processor use over 60 idle seconds. The app measured is the one whose
terminal runs the script: SSHeesh 0.10.x or the prototype.

The result is a short text file in a `results` folder beside the scripts: numbers, the system's
version, the processor and graphics card models. No computer name, user name, address or path.
Test files go to the system's temporary folder; `clean` removes them. The scripts install
nothing, change no settings and make no network requests.

`measure.sh` and `run-prototype.sh` were run on Linux in both apps. The PowerShell scripts and
`measure.sh` on macOS have not been run yet.
