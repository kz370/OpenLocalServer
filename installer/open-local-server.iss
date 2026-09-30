; The AppId is one string in two places: [Setup] below, and the uninstall
; registry key the [Code] section reads to find where the app really is. A
; define is what keeps them equal -- AppMutex in [Setup] is a third place, but
; that one is owned by tauri.conf.json's `identifier` and cannot be derived
; from here, so the comment on it is the link.
#define AppId "dev.openlocalserver.app"
#define AppName "OLS"
#ifndef AppVersion
  #define AppVersion "0.0.1"
#endif
#ifndef SourceExe
  #define SourceExe "..\release\OLS.exe"
#endif
#ifndef LibDir
  #define LibDir "..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\release"
#endif

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=OpenLocalServer contributors
; The default is a fixed, obvious folder rather than {autopf}, which resolves to
; a per-user path under AppData and so depends on who is installing. A fixed
; C:\OpenLocalServer is the same place for everyone, which matters most for an
; upgrade: the folder is written into the registry, and paths under a roaming
; profile move with the user while this one does not.
;
; The privilege mode is admin rather than lowest BECAUSE of that default: a
; per-user install runs unelevated and cannot create C:\OpenLocalServer, so the
; two settings only work together. The override is still offered, so somebody
; who genuinely cannot elevate can install per user -- and the [Code] section
; then detects that second copy and refuses to leave two of them behind.
DefaultDirName=C:\OpenLocalServer
DefaultGroupName={#AppName}
DisableDirPage=no
; Keep the previous install's directory so an upgrade lands on top of the
; old one. This is the "unless the app is already installed, use where it
; actually is" half of the rule; DefaultDirName is the "otherwise" half, and
; the [Code] section checks the two against each other.
;
; Note this means ANY earlier install of this AppId seeds the next run's
; default, even a test install into a scratch folder or TEMP -- verified here:
; a silent install with /DIR= into a temp folder recorded that folder, and the
; next run offered it as the destination, silently repointing a real install's
; registry entry. Run that install's unins000.exe (or delete its uninstall
; registry entry) after any such test. AskAboutExistingInstall is what turns
; that silent repointing into a question.
UsePreviousAppDir=yes
PrivilegesRequired=admin
PrivilegesRequiredOverridesAllowed=commandline dialog
AllowNoIcons=yes
DisableProgramGroupPage=no
OutputDir={#OutputDir}
OutputBaseFilename=OLS-{#AppVersion}-setup
UninstallDisplayIcon={app}\OLS.exe
SetupIconFile=..\src-tauri\icons\icon.ico
Compression=lzma
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
WizardStyle=modern
; An upgrade could not replace the app while it was running: Windows locks a
; running image, so the installer would either fail on [Files] or leave the old
; exe in place and the "update" would silently do nothing.
;
; Two layers close it:
;   1. The [Code] section, which is the one that matters. It asks once, with
;      an explanation, and then closes the processes that actually hold a file
;      in the destination folder -- matched by PATH, not by name, because a
;      process by the same name from a different install is not in the way. It
;      has to: the app has a second process that holds a very binary being
;      overwritten and ships no file under the app's own name, `olsc.exe`
;      running in daemon mode, auto-spawned detached by the CLI
;      (crates/ols-cli/src/main.rs, spawn_daemon). Restart Manager is only told
;      about the files in [Files], so it never learns about that one.
;   2. CloseApplications below, as a silent backstop for anything layer 1
;      missed.
;
; The pruning in [Code] at ssPostInstall is the third half: a DLL or exe from an
; older version that this version no longer ships would otherwise stay in the
; folder and keep being loaded, which is the other way an "updated" app goes on
; running old code.
;
; AppMutex was here first and was removed. It is the wrong tool twice over:
; Restart Manager cannot see the daemon (above), and AppMutex makes Setup put up
; its own "Setup has detected that OLS is currently running"
; prompt -- a second question about a thing the [Code] section has already asked
; about with an explanation attached. Under /SUPPRESSMSGBOXES that prompt
; silently defaults to Cancel and the install dies there, which is how it was
; found: the log recorded the user answering Yes to the first prompt and the
; second one aborting immediately after. The mutex also only ever matched the
; GUI, because tauri-plugin-single-instance holds "{identifier}-sim" and the
; daemon never touches it.
;
; `force`, not `yes`, for the same reason: `yes` brings back that prompt.
CloseApplications=force
CloseApplicationsFilter=*.exe|*.dll
; Relaunch whatever Restart Manager closed, so the app the user was using comes
; back after an upgrade instead of leaving them at a desktop.
RestartApplications=yes

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
; The command line. It has to ship: the Explorer right-click menu is written to run
; it (shell_menu.rs find_cli), and find_cli only looks beside the app or on PATH --
; so without this file the menu could not be installed at all, and on a machine where
; the app folder is not on PATH it could not work either. Named `olsc.exe`, not `ols.exe`, because
; the app itself is OLS.exe and Windows would not tell the two apart otherwise.
Source: "{#LibDir}\olsc.exe"; DestDir: "{app}"; Flags: ignoreversion
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
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\OLS.exe"; IconFilename: "{app}\icon.ico"; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\OLS.exe"; IconFilename: "{app}\icon.ico"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
; The "Launch OLS" checkbox on the final page, ticked by default.
; `postinstall` is what draws the checkbox at all -- without it this is a silent
; post-install run. It points at {app}, never at a shortcut, so the copy that
; was just installed is the one that opens; a shortcut can still point at an
; older install directory and would launch the version this installer just
; replaced. The checkbox is checked by default -- `postinstall` draws it, and
; the way to uncheck it is Inno's `unchecked` flag, not a `Check:` line
; (`Check:` is a task condition and has nothing to do with the checkbox).
; `skipifsilent` because a silent install has no checkbox to drive, and
; CurStepChanged relaunches for that case instead. No `runhidden`: the app
; shows its own window, and hiding the process would leave the user with no
; sign that anything started.
Filename: "{app}\{#AppName}.exe"; Description: "Launch {#AppName}"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent


[Code]
{ Stop the processes that hold a file this installer has to overwrite, find the
  directory the app is really installed in, and remove what an older version
  left behind. Written as three named steps because the failure this prevents
  ("the update says it worked and the old version is still there") has three
  causes that look identical from the outside. }

const
  AppExe      = 'OLS.exe';
  { The name this app shipped under before the rename. It is still the image
    name of every install made before it, and it is the name of the file a
    pre-rename build is running right now -- so every check below that asks
    "is the app running?" has to ask about both. Asking only about OLS.exe
    means an upgrade onto a pre-rename install does not see the process
    holding the very exe it is about to overwrite, and the install reports
    success while the old version stays on disk. The legacy file is removed
    by PruneUnshippedFiles at ssPostInstall, which is the whole migration. }
  LegacyAppExe = 'OLS.exe';
  { The background service. It is the *CLI* in daemon mode, spawned detached by
    `olsc.exe daemon` (crates/ols-cli/src/main.rs, spawn_daemon), so its image is
    olsc.exe -- it was AppExe here, which meant the daemon was never actually
    detected and the app was asked about twice. }
  DaemonExe   = 'olsc.exe';
  HelperExe   = 'ols-helper.exe';
  { Kept in the prune allow-list at ssPostInstall, or the prune would delete the
    shim [Files] has just written. }
  LimiterExe  = 'cpulimit.exe';
  { The Start Menu folder an install made before the rename created. The group
    name is NOT read from DefaultGroupName on an upgrade: Inno takes it from the
    previous install's uninstaller (unins000.exe carries the name), so it runs
    the old uninstaller to clear the old entries and then puts the new shortcut
    back into the same folder. That is why a machine upgraded from the
    pre-rename build keeps a Start Menu folder called "Open Local Server" and
    the new shortcut lands in it -- the folder is the only user-visible place
    the old name survives, and no [Setup] setting can change it, because it
    would have to change the name the OLD uninstaller deletes from.
    MigrateLegacyStartMenuGroup moves it after the install instead. }
  LegacyGroupName = 'Open Local Server';
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{#AppId}_is1';
  { A const, not #13#10 written inline: ISPP reads a line whose first character
    is # as a preprocessor directive, so a wrapped Pascal expression must never
    put a #13#10 at the start of a line. Wrapping for readability is fine; only
    the leading character is not. }
  NL = #13#10;

{ There is deliberately no top-level `var` here. ISCC 7 rejects a script that
  declares one and then defines a support function -- "Invalid prototype for
  'InitializeWizard'" -- and it reads as a complaint about the function rather
  than about the declaration above it. The one value that used to be shared
  between InitializeSetup and InitializeWizard is read from the registry by
  each, which is two cheap queries and also means the check still runs if the
  user changes the folder on the directory page. }

{ Reads InstallLocation from one registry root. The uninstall key lives in
  HKCU for a per-user install and in HKLM for a machine-wide one, and
  PrivilegesRequired=lowest with PrivilegesRequiredOverridesAllowed lets a
  machine have both at once -- so a lookup that only checks one root will
  "detect" nothing and silently install a second copy next to the first. }
function RegisteredDir(RootKey: Integer): String;
var
  Dir: String;
begin
  Result := '';
  if RegQueryStringValue(RootKey, UninstallKey, 'InstallLocation', Dir) then
  begin
    Dir := Trim(Dir);
    { Trust the key only if the app is actually in it. A test install into a
      scratch folder or TEMP records that folder forever (UsePreviousAppDir),
      and installing on top of it would leave the real install untouched --
      which is precisely the bug this whole change set is about. Both names
      count as the app: a machine whose install predates the rename holds
      LegacyAppExe, and testing only for OLS.exe would report that install as
      absent -- which is the one answer that silently produces two copies. }
    if (Dir <> '') and
       (FileExists(AddBackslash(Dir) + AppExe) or
        FileExists(AddBackslash(Dir) + LegacyAppExe)) then
      Result := Dir;
  end;
end;

function CurrentInstalledDir: String;
var
  MachineDir: String;
begin
  MachineDir := RegisteredDir(HKLM);
  if MachineDir <> '' then
  begin
    Result := MachineDir;
    Exit;
  end;
  Result := RegisteredDir(HKCU);
end;

{ Is an image running right now?
  Deliberately NOT taskkill's exit code: taskkill exits 0 every time it
  successfully *sends* the signal, so a poll built on it reads a process that
  is still alive as "stopped", and the install then goes on to fail on a locked
  file -- the exact failure this section exists to remove. tasklist is asked
  instead, and the name it prints is the image name, which is what /IM wants. }
function ImageRunning(const ImageName: String): Boolean;
var
  Tasklist: String;
  TmpFile: String;
  Content: AnsiString;
  ResultCode: Integer;
begin
  Tasklist := ExpandConstant('{sys}\tasklist.exe');
  TmpFile := ExpandConstant('{tmp}\ols-tasklist.txt');
  { taskkill's exit code cannot answer this -- it exits 0 every time it
    successfully *sends* a signal, so a process that is still alive looks
    stopped, and the install then fails on a locked file, which is the exact
    failure this section exists to remove. tasklist can answer it, but Exec
    does not return stdout and this version of Setup has no TExecResult, so the
    output is redirected to a file and read back. /NH suppresses the header;
    with no match tasklist writes an INFO line naming no image. }
  Exec(ExpandConstant('{cmd}'), '/C ""' + Tasklist + '" /FI "IMAGENAME eq ' +
       ImageName + '" /NH > "' + TmpFile + '" 2>NUL"', '', SW_HIDE,
       ewWaitUntilTerminated, ResultCode);
  if not LoadStringFromFile(TmpFile, Content) then
  begin
    { Could not tell. Guessing "not running" would skip the confirmation and go
      straight to a failed install, so the safe answer is "yes, treat it as
      running" and let the user decide. }
    Result := True;
    Exit;
  end;
  Result := Pos(Uppercase(ImageName), Uppercase(Content)) > 0;
end;

{ There is no powershell constant in Setup -- ExpandConstant raises an
  "Unknown constant" internal error and takes the whole install down, which is
  what it did here.

  The ORDER matters more than it looks, and getting it wrong is invisible. A
  32-bit Setup sees SysWOW64 as the sys directory, so asking for the sys path
  first finds the 32-bit PowerShell -- and a 32-bit process cannot read the
  MainModule path of a 64-bit process, so `$_.Path` comes back empty for the
  app, the filter matches nothing, and the query answers "0 processes running"
  for an app that is running. The installer then concluded there was nothing to
  close, skipped the kill, and walked into the locked file: "DeleteFile failed;
  code 5. Access is denied" on the app's own exe. Both answers were verified
  side by side on this machine -- 64-bit PowerShell says 1, 32-bit says 0.
  The app is x64 and this Setup is 32-bit, so the 64-bit interpreter goes first
  and the 32-bit one is only a fallback for a machine that has no other. }
function PowerShellPath: String;
var
  Attempt: Integer;
begin
  Result := '';
  for Attempt := 1 to 2 do
  begin
    if Attempt = 1 then
      Result := ExpandConstant('{win}\System32\WindowsPowerShell\v1.0\powershell.exe')
    else
      Result := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
    if FileExists(Result) then
      Exit;
  end;
end;

{ How many processes are running FROM this install folder with this image name.
  The path is the whole question: only a process whose image is inside the
  folder being installed to can be holding a file this installer has to write.
  A helper service running from ANOTHER install -- which is the normal state of
  a machine that has two installs, and this one did -- holds no lock on this
  one, and refusing an update for it would be a false alarm.
  The comparison is done in PowerShell rather than here because Exec does not
  return stdout, this version of Setup has no TExecResult, and reading the
  result back as a file fights the script engine's string types. The answer
  comes back as one number, which is all that is needed.
  Returns 1 when it could not tell: an unknown answer must not be read as
  "nothing is running", which is how a locked file turns into a failed install. }
function ProcessesInApp(const ImageName: String): Integer;
var
  Script: String;
  OutFile: String;
  Content: AnsiString;
  ResultCode: Integer;
  Q: String;
  Ps: String;
begin
  Q := '''';
  Ps := PowerShellPath;
  if Ps = '' then
  begin
    Result := 1;
    Exit;
  end;
  OutFile := ExpandConstant('{tmp}\ols-processes.txt');
  DeleteFile(OutFile);
  Script := '$d = ' + Q + Lowercase(AddBackslash(ExpandConstant('{app}'))) + Q + ';' +
            '(Get-Process -ErrorAction SilentlyContinue | Where-Object {' +
            ' $_.Path -and $_.Path.ToLower().StartsWith($d) -and ' +
            '[IO.Path]::GetFileName($_.Path).ToLower() -eq ' + Q +
            Lowercase(ImageName) + Q + ' } | Measure-Object).Count' +
            ' | Set-Content -LiteralPath ' + Q + OutFile + Q;
  { PowerShell is run directly and writes its own output file. Going through
    `cmd /C` with a `>` redirect looked equivalent and was not: the pipe and
    the braces end up inside a nested quoted command line, the query came back
    with the wrong answer, and the installer concluded nothing was running and
    walked into the locked file. The log line that gave it away read
    "processes running from the install folder ... = 0" for an app that was
    demonstrably running from that folder. }
  Log('ols: query = ' + Script);
  Exec(Ps, '-NoProfile -NonInteractive -Command "' + Script + '"', '',
       SW_HIDE, ewWaitUntilTerminated, ResultCode);
  if not LoadStringFromFile(OutFile, Content) then
  begin
    Log('ols: no process list for ' + ImageName + ' (powershell=' + Ps +
        ', exit=' + IntToStr(ResultCode) + '); treating it as running');
    Result := 1;
    Exit;
  end;
  Content := Trim(Content);
  { No set membership here: this Pascal dialect has no `in ['0'..'9']`, and
    the compiler says so by pointing at the closing bracket. }
  if (Length(Content) = 0) or (Content[1] < '0') or (Content[1] > '9') then
  begin
    Log('ols: unreadable process count for ' + ImageName + ' (got "' +
        String(Content) + '"); treating it as running');
    Result := 1;
    Exit;
  end;
  Result := StrToIntDef(String(Content), 1);
  Log('ols: processes running from the install folder as ' + ImageName +
      ' = ' + IntToStr(Result));
end;

function ImageRunningInApp(const ImageName: String): Boolean;
begin
  Result := ProcessesInApp(ImageName) > 0;
end;

{ Stops one image, but only if it is the one inside this install folder.
  Forcibly, and without the polite WM_CLOSE first: the app is a tray app whose
  window close handler hides it instead of quitting, so WM_CLOSE does not stop
  it -- it was tried, and the process was still there a minute later. There is
  no quit message an installer can send to a Tauri window short of killing it.
  True once the image is gone, or when it was never ours to begin with. }
function StopAppImage(const ImageName: String): Boolean;
var
  ResultCode: Integer;
  Attempts: Integer;
  Taskkill: String;
begin
  if not ImageRunningInApp(ImageName) then
  begin
    Result := True;
    Exit;
  end;
  Taskkill := ExpandConstant('{sys}\taskkill.exe');
  Log('ols: stopping ' + ImageName + ' from ' + ExpandConstant('{app}'));
  Exec(Taskkill, '/F /IM "' + ImageName + '"', '', SW_HIDE,
       ewWaitUntilTerminated, ResultCode);
  Log('ols: taskkill exit=' + IntToStr(ResultCode));
  for Attempts := 1 to 20 do
  begin
    if not ImageRunningInApp(ImageName) then
    begin
      Log('ols: ' + ImageName + ' closed after ' + IntToStr(Attempts) +
          ' check(s)');
      Result := True;
      Exit;
    end;
    Sleep(500);
  end;
  Log('ols: ' + ImageName + ' would not close');
  Result := not ImageRunningInApp(ImageName);
end;

{ Which of the images that can hold a file in THIS folder are running. Names
  only, for the message. }
function AnyAppRunningHere: String;
begin
  Result := '';
  if ImageRunningInApp(LegacyAppExe) then
    Result := LegacyAppExe;
  if ImageRunningInApp(AppExe) then
  begin
    if Result <> '' then
      Result := Result + ' and ';
    Result := Result + AppExe;
  end;
  if ImageRunningInApp(DaemonExe) then
  begin
    if Result <> '' then
      Result := Result + ' and ';
    Result := Result + DaemonExe;
  end;
  if ImageRunningInApp(HelperExe) then
  begin
    if Result <> '' then
      Result := Result + ' and ';
    Result := Result + HelperExe;
  end;
end;

{ Kill one image by name globally (no path filter), used in InitializeSetup
  before the app constant is resolved. The path-filtered variant used in
  PrepareToInstall is still the one that matters for locked files, but the
  early kill here is what makes the wizard feel responsive instead of leaving
  the app alive while the user clicks through pages.

  Like StopAppImage it polls instead of trusting taskkill's exit code:
  taskkill exits 0 every time it successfully sends the signal, so a process
  that is still shutting down looks stopped and the install walks into the
  locked file. 20 x 500 ms = 10 s maximum wait, same as StopAppImage. }
procedure KillByImageNameGlobal(const ImageName: String);
var
  Taskkill: String;
  ResultCode: Integer;
  Attempts: Integer;
begin
  if not ImageRunning(ImageName) then
    Exit;
  Taskkill := ExpandConstant('{sys}\taskkill.exe');
  Log('ols: early kill of ' + ImageName + ' (pre-wizard)');
  Exec(Taskkill, '/F /IM "' + ImageName + '"', '', SW_HIDE,
       ewWaitUntilTerminated, ResultCode);
  Log('ols: taskkill exit=' + IntToStr(ResultCode));
  for Attempts := 1 to 20 do
  begin
    if not ImageRunning(ImageName) then
    begin
      Log('ols: ' + ImageName + ' gone after ' + IntToStr(Attempts) + ' check(s)');
      Exit;
    end;
    Sleep(500);
  end;
  Log('ols: ' + ImageName + ' still running after early kill (PrepareToInstall will retry)');
end;

{ The one question, asked before the wizard appears, so the processes are gone
  by the time the user reads the next page and no window vanishes from under
  them mid-question. Answering No cancels the install outright. The actual
  stopping happens in PrepareToInstall, where the destination is known and
  therefore only the processes in that destination can be identified. }
function InitializeSetup(): Boolean;
var
  Running: String;
begin
  { Before the directory is known, this can only ask about the image names --
    so it is deliberately phrased as a question, not a claim. LegacyAppExe is
    in the test because an install that predates the rename is running under
    that name, and it holds the file [Files] is about to write. }
  if not (ImageRunning(AppExe) or ImageRunning(LegacyAppExe) or
          ImageRunning(DaemonExe)) then
  begin
    Result := True;
    Exit;
  end;
  Result := MsgBox('OLS is running and has to be closed to ' +
    'install.' + NL + NL +
    'It will be closed, along with its services, and the installer will ' +
    'continue. Anything it has not finished is reported the next time it ' +
    'starts.' + NL + NL + 'Close it and continue?', mbConfirmation,
    MB_YESNO) = IDYES;
  { Close the processes immediately so the wizard pages are clear of any
    running app window. The app constant is not resolved here, so the
    path-filtered StopAppImage cannot be used -- KillByImageNameGlobal kills
    by name only. PrepareToInstall repeats the check path-filtered; this is
    the early kill. }
  if Result then
  begin
    KillByImageNameGlobal(LegacyAppExe);
    KillByImageNameGlobal(AppExe);
    KillByImageNameGlobal(DaemonExe);
    KillByImageNameGlobal(HelperExe);
  end;
end;

{ The two-copies problem, and the only place it is actually fixed.
  UsePreviousAppDir=yes already makes the app directory the previous install's
  folder, so in the ordinary upgrade there is nothing to do and nothing is
  asked. The guard exists for the case where those two disagree -- a per-user
  install next to a machine-wide one, or a test install whose recorded folder
  is a scratch directory -- which produces an update that reports success while
  the copy the user launches is untouched.

  It must not merely *ask*: an earlier version of this asked a Yes/No question
  whose Yes button changed nothing, because the answer was ignored and the
  install went to the directory the user had just been told it would not go to.
  Answering Yes on the directory page sets the page's edit box, so the answer
  is the behaviour.

  Note on the API, since it is not guessable: in this version of Setup
  `WizardDirValue` and `WizardForm.WizardDirValue` do not exist (ISCC answers
  "Unknown identifier"); the way to redirect is `WizardForm.DirEdit`, which the
  directory page reads on the way out. `NextButtonClick` keeps its Boolean
  result, but `InitializeWizard` and `CurStepChanged` do not have one, and
  declaring those as functions is rejected as an "Invalid prototype" -- an
  error that points at the function name and not at the signature. }
function AskAboutExistingInstall(AllowRedirect: Boolean): Boolean;
var
  DetectedDir: String;
  Target: String;
begin
  DetectedDir := CurrentInstalledDir;
  if DetectedDir = '' then
  begin
    Result := True;
    Exit;
  end;
  if AllowRedirect then
    Target := RemoveBackslash(WizardForm.DirEdit.Text)
  else
    Target := RemoveBackslash(ExpandConstant('{app}'));
  if CompareText(Target, RemoveBackslash(DetectedDir)) = 0 then
  begin
    Result := True;
    Exit;
  end;
  if AllowRedirect then
    Result := MsgBox('OLS is already installed in' + NL + NL +
      DetectedDir + NL + NL +
      'Install over that copy, instead of the folder on the next page?' + NL +
      NL + 'Installing somewhere else leaves two copies, and the one you ' +
      'keep starting is the one that would not have been updated.',
      mbConfirmation, MB_YESNO) = IDYES
  else
    Result := MsgBox('OLS is already installed in' + NL + NL +
      DetectedDir + NL + NL +
      'This installer is installing into a different folder:' + NL + NL +
      Target + NL + NL +
      'The copy you keep starting will be left exactly as it is, so this ' +
      'install will have updated nothing you can see.' + NL + NL +
      'Install a second copy anyway?', mbConfirmation, MB_YESNO) = IDYES;
  { On the directory page, a refusal still has to answer the question the page
    asked, so redirect anyway -- that is the only outcome that is not a second
    copy. Once the page is behind us, a refusal is a refusal. }
  if AllowRedirect and not Result then
  begin
    WizardForm.DirEdit.Text := DetectedDir;
    Result := True;
  end;
end;

{ Pre-populate the directory page with the exe-verified install path so that
  (a) a first-time install shows the fixed C:\OpenLocalServer default and
  (b) an upgrade lands on the folder where the app actually is, not on wherever
  UsePreviousAppDir last recorded -- which can be a stale scratch directory.

  This is the "dynamic installation path" logic:
    - App detected  -> use the existing install directory (exe-verified).
    - App not found -> leave the default (C:\OpenLocalServer from [Setup]).

  It runs once, before the wizard renders the first page, so the directory
  page already shows the right value when the user reaches it.  The check in
  AskAboutExistingInstall / NextButtonClick remains as a second guard in case
  the user overrides the pre-filled path by hand. }
procedure InitializeWizard;
var
  DetectedDir: String;
begin
  DetectedDir := CurrentInstalledDir;
  if DetectedDir <> '' then
  begin
    Log('ols: existing install found at "' + DetectedDir +
        '"; pre-seeding directory page');
    WizardForm.DirEdit.Text := DetectedDir;
  end
  else
    Log('ols: no existing install found; using default directory');
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if CurPageID = wpSelectDir then
    Result := AskAboutExistingInstall(True);
end;

{ The same check for an install that never showed the directory page, because
  it was given /DIR= on the command line. There is nothing to redirect to by
  then, so the answer is taken at its word: No aborts.

  This runs from PrepareToInstall and NOT from ssInstall, and that placement is
  load-bearing. ssInstall fires after the old version's files have already been
  removed, so aborting there does not cancel the install -- it leaves a machine
  with the old files gone and the new ones never written, which is a strictly
  worse outcome than the two copies it was trying to prevent. Verified by
  running it: an aborted upgrade reduced the install to a single exe. }
{ Everything that has to be true before a single byte is written, and the only
  place a refusal is still free.
  Placement is load-bearing in both directions:
   - It cannot be earlier. The app constant is not initialized until the
     directory page is resolved, so expanding it in InitializeWizard aborts the
     whole install with "An attempt was made to expand the 'app' constant
     before it was initialized" -- hit by running this, not by reading it.
   - It cannot be ssInstall. That fires after the old version's files have
     already been removed, so aborting there does not cancel the install, it
     leaves a machine with the old files gone and the new ones never written.
     Verified by running it: an aborted upgrade reduced the install to one exe. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Stubborn: String;
begin
  Result := '';
  NeedsRestart := False;
  Log('ols: PrepareToInstall, destination ' + ExpandConstant('{app}'));
  { Close what is running out of THIS folder, and only that. Another install
    elsewhere on the machine -- including the auto-start helper service, which
    runs from Program Files and is not even in the file list being written --
    is left strictly alone. }
  Stubborn := '';
  if not StopAppImage(LegacyAppExe) then
    Stubborn := LegacyAppExe;
  if not StopAppImage(AppExe) then
  begin
    if Stubborn <> '' then
      Stubborn := Stubborn + ' and ';
    Stubborn := Stubborn + AppExe;
  end;
  if not StopAppImage(DaemonExe) then
  begin
    if Stubborn <> '' then
      Stubborn := Stubborn + ' and ';
    Stubborn := Stubborn + DaemonExe;
  end;
  if not StopAppImage(HelperExe) then
  begin
    if Stubborn <> '' then
      Stubborn := Stubborn + ' and ';
    Stubborn := Stubborn + HelperExe;
  end;
  if Stubborn <> '' then
  begin
    Result := 'OLS could not be closed: ' + Stubborn +
      ' is still running from ' + ExpandConstant('{app}') + '.' + NL + NL +
      'Its files are locked. Close it from the tray or the taskbar and run ' +
      'the installer again -- installing now would leave the old version in ' +
      'place while reporting that the update succeeded.';
    Exit;
  end;
  if not AskAboutExistingInstall(False) then
    Result := 'Stopped: installing into ' + ExpandConstant('{app}') +
      ' would leave the copy you already use, in ' +
      CurrentInstalledDir + ', exactly as it is.';
end;

{ After the new files are in place, drop anything this version no longer ships.
  A leftover DLL from an older version keeps being loaded out of the app
  directory, and an
  upgrade that never removes a file it stopped shipping is how an "updated" app
  goes on running old code. Deliberately narrow: files only (never recursing --
  a portable install keeps its data\ folder beside the exe and deleting that
  would destroy the user's projects) and only the two extensions this installer
  ever writes.

  This is also the whole migration for the exe rename, and it is worth being
  explicit about why it is safe: LegacyAppExe is deliberately NOT in the
  Shipped list below, so an install that predates the rename has that file
  deleted here -- at ssPostInstall, which only runs after PrepareToInstall
  confirmed the image is closed, so the delete cannot hit a locked file. Left
  in place it would be worse than untidy: two OLS exes in one folder, the old
  one still holding the single-instance mutex, and the user's existing Start
  menu shortcut still launching the version that was just replaced. }
procedure PruneUnshippedFiles;
var
  Found: TFindRec;
  Shipped: String;
  Path: String;
begin
  Shipped := '|' + Lowercase(AppExe) + '|' + Lowercase(DaemonExe) + '|' +
             Lowercase(HelperExe) + '|' + Lowercase(LimiterExe) + '|' +
             'icon.ico|unins000.exe|unins000.dat|';
  if FindFirst(ExpandConstant('{app}\*'), Found) then
  begin
    try
      repeat
        { The extension test has to exclude the empty extension explicitly.
          Pos('', '.dll.exe') is 1 -- an empty needle is found at the start --
          so without this the walk matches "." and ".." and the log fills with
          "Could not remove the leftover file C:\...\." on every install. }
        if (ExtractFileExt(Found.Name) <> '') and
           (Pos(Lowercase(ExtractFileExt(Found.Name)), '.dll.exe') > 0) and
           (Pos('|' + Lowercase(Found.Name) + '|', Shipped) = 0) then
        begin
          Path := ExpandConstant('{app}\') + Found.Name;
          { A file still locked here is logged, not silently skipped: it is the
            same class of bug as the one this section exists to fix, and the
            setup log is the only place it will ever show up. }
          if not DeleteFile(Path) then
            Log('Could not remove the leftover file ' + Path);
        end;
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
end;

{ Move the shortcuts out of the pre-rename Start Menu folder and delete it.
  (Constants cannot be named inside a brace comment -- the closing brace of
  one ends the comment -- so the group folder is described, not written.)

  Runs at ssPostInstall, so [Icons] has already put the new shortcut in the
  group folder and the old uninstaller has already removed its entry from the
  legacy folder -- which is why the move can only ever be a collision with a
  shortcut the user created, never with one this installer owns twice.

  Deliberately narrow, because Start Menu folders are the user's space:
    - only .lnk files, so a document folder the user filed there is left
      exactly as it is;
    - a name that already exists in the group folder is deleted rather than
      overwritten, since this installer's copy is the one [Icons] just wrote;
    - the legacy folder is removed only when it ends up empty, and never
      recursively -- the empty folder is the cosmetic leftover being cleaned,
      and a folder holding anything the user put there must survive.
  Failures are logged, not raised: an installer that fails to tidy the Start
  Menu has still installed the app correctly, and aborting here would undo a
  successful install over a cosmetic problem. }
procedure MigrateLegacyStartMenuGroup;
var
  LegacyDir: String;
  GroupDir: String;
  Entry: String;
  Found: TFindRec;
  Empty: Boolean;
begin
  GroupDir := RemoveBackslash(ExpandConstant('{group}'));
  LegacyDir := AddBackslash(ExtractFileDir(GroupDir)) + LegacyGroupName;
  { A machine whose group name never moved -- an install where DefaultGroupName
    was already OLS -- puts the two on the same path. Nothing to migrate, and
    without this every shortcut would be renamed onto itself. }
  if CompareText(LegacyDir, GroupDir) = 0 then
    Exit;
  if not DirExists(LegacyDir) then
    Exit;
  if FindFirst(AddBackslash(LegacyDir) + '*.lnk', Found) then
  begin
    try
      repeat
        Entry := AddBackslash(LegacyDir) + Found.Name;
        if FileExists(AddBackslash(GroupDir) + Found.Name) then
          DeleteFile(Entry)
        else if not RenameFile(Entry, AddBackslash(GroupDir) + Found.Name) then
          Log('Could not move the legacy Start Menu shortcut ' + Entry);
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
  Empty := True;
  if FindFirst(AddBackslash(LegacyDir) + '*', Found) then
  begin
    try
      repeat
        if (Found.Name <> '.') and (Found.Name <> '..') then
        begin
          Empty := False;
          Break;
        end;
      until not FindNext(Found);
    finally
      FindClose(Found);
    end;
  end;
  if Empty then
  begin
    if not RemoveDir(LegacyDir) then
      Log('Could not remove the empty legacy Start Menu folder ' + LegacyDir);
  end
  else
    Log('Legacy Start Menu folder left in place, it is not empty: ' +
        LegacyDir);
end;

{ A procedure, for the same reason as InitializeWizard above: in this version of
  Setup the event has no Boolean result, and a `function` form is rejected as an
  invalid prototype. }
procedure CurStepChanged(CurStep: TSetupStep);
var
  ResultCode: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    PruneUnshippedFiles;
    MigrateLegacyStartMenuGroup;
    { The [Run] entry below is skipped on a silent install, and the in-app
      update is exactly that case -- updater::install_update spawns the setup
      with no wizard at all -- so nothing would bring the app back. Doing it
      here covers both paths. A second launch is harmless either way: the app
      is single-instance, so it focuses the window that is already open. }
    if WizardSilent then
      Exec(ExpandConstant('{app}\') + AppExe, '', ExpandConstant('{app}'),
           SW_SHOWNORMAL, ewNoWait, ResultCode);
  end;
end;