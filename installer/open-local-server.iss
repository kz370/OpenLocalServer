#define AppName "Open Local Server"
#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif
#ifndef SourceExe
  #define SourceExe "..\release\Open Local Server.exe"
#endif
#ifndef LibDir
  #define LibDir "..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\release"
#endif

[Setup]
AppId=dev.openlocalserver.app
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=OpenLocalServer contributors
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableDirPage=no
; Keep the previous install's directory so an upgrade lands on top of the
; old one. Note this means ANY earlier install of this AppId seeds the next
; run's default, even a test install into a scratch folder or TEMP -- verified
; here: a silent install with /DIR= into a temp folder recorded that folder and
; the next run offered it as the destination. Run that install's unins000.exe
; (or delete its uninstall registry entry) after any such test.
UsePreviousAppDir=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
AllowNoIcons=yes
DisableProgramGroupPage=no
OutputDir={#OutputDir}
OutputBaseFilename=Open-Local-Server-{#AppVersion}-setup
UninstallDisplayIcon={app}\Open Local Server.exe
SetupIconFile=..\src-tauri\icons\icon.ico
Compression=lzma
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
; An upgrade could not replace the app while it was running: Windows locks a
; running image, so the installer would either fail on [Files] or leave the old
; exe in place and the "update" would silently do nothing. These three settings
; are the whole fix. Inno asks Restart Manager to close the app, waits for the
; lock to clear, and relaunches the new copy when it is done.
;
; AppMutex is NOT an arbitrary name: tauri-plugin-single-instance creates a
; named mutex "{identifier}-sim" (see the plugin's platform_impl\windows.rs),
; and the app identifier is dev.openlocalserver.app. Rename either and this
; stops matching, so the app runs through the upgrade again and the files are
; never replaced. The ols-helper.exe that ships beside it is not part of the
; mutex, which is why it is in the close filter too.
AppMutex=dev.openlocalserver.app-sim
CloseApplications=yes
CloseApplicationsFilter=*.exe|*.dll|ols-helper.exe|cpulimit.exe
RestartApplications=yes
RestartApplicationsIfNeededByRun=no

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#LibDir}\ols-helper.exe"; DestDir: "{app}"; Flags: ignoreversion
; The CPU limiter for Settings > Resources (§129). Shipped next to the app because
; resources::find_cpu_limiter looks in the current exe's directory first, so a
; default install can cap CPU with nothing configured.
Source: "{#OutputDir}\cpulimit.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#LibDir}\*.dll"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist
Source: "..\src-tauri\icons\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\Open Local Server.exe"; IconFilename: "{app}\icon.ico"; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\Open Local Server.exe"; IconFilename: "{app}\icon.ico"; WorkingDir: "{app}"; Tasks: desktopicon