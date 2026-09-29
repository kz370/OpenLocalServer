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
UsePreviousAppDir=yes
AppendDefaultDirName=yes
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