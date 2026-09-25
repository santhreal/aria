; Inno Setup script for the Aria Windows installer.
;
; This compiles the staged payload produced by scripts/package-windows.ps1
; (aria-desktop.exe + aria.ico + licenses + README) into a single Setup.exe
; with a Start-menu shortcut, an optional desktop icon, the .aria file
; association, and an uninstaller.
;
; Do not run this file directly; use scripts/package-windows-installer.ps1,
; which builds, stages, and invokes ISCC with the right defines. To compile
; by hand:
;
;   iscc /DMyAppVersion=0.5.3-alpha.0 ^
;        "/DMySourceDir=C:\path\to\dist\staging-windows-x86_64\aria-0.5.3-alpha.0-windows-x86_64" ^
;        "/DMyOutputDir=C:\path\to\dist" ^
;        packaging\windows\aria.iss

#define MyAppName "Aria"
#define MyAppExeName "aria-desktop.exe"
#define MyAppPublisher "Aria"
#define MyAppURL "https://github.com/santhreal/aria"
; The mark, as a multi-size .ico built from assets/icon/aria-icon.png. Used for
; the setup wizard, the shortcuts, and the project file type: the executable
; carries no icon resource, so every icon a user sees points at this file.
#define MyAppIcon "aria.ico"
; Project file type. `aria-desktop` takes paths as arguments and opens a
; project or imports media, which is what a double-click and "Open With" send.
#define MyAssocExt ".aria"
#define MyAssocKey "Aria.Project"

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif

#ifndef MySourceDir
  #define MySourceDir "..\..\dist\staging-windows-x86_64\aria-" + MyAppVersion + "-windows-x86_64"
#endif

#ifndef MyOutputDir
  #define MyOutputDir "..\..\dist"
#endif

#ifndef MyOutputBaseFilename
  #define MyOutputBaseFilename "Aria-" + MyAppVersion + "-windows-x86_64-Setup"
#endif

; Inno Setup architecture identifier: "x64compatible" or "arm64".
#ifndef MyArchAllowed
  #define MyArchAllowed "x64compatible"
#endif

[Setup]
; AppId uniquely identifies this product for upgrades/uninstall. Keep it stable.
AppId={{B6E6F3C2-7F4D-4E2A-9C3E-9A0F2C1D4E7B}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
LicenseFile={#MySourceDir}\LICENSE-MIT
OutputDir={#MyOutputDir}
OutputBaseFilename={#MyOutputBaseFilename}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Aria ships only as 64-bit; install into the native Program Files.
ArchitecturesAllowed={#MyArchAllowed}
ArchitecturesInstallIn64BitMode={#MyArchAllowed}
UninstallDisplayIcon={app}\{#MyAppIcon}
#ifdef MyAppIcon
SetupIconFile={#MyAppIcon}
#endif
; The project file type is written and removed with the app, so Explorer is
; told to re-read its icon cache on both.
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#MySourceDir}\*"; DestDir: "{app}"; Flags: recursesubdirs createallsubdirs ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIcon}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; IconFilename: "{app}\{#MyAppIcon}"; Tasks: desktopicon

; Double-clicking a project opens it in the editor, with the mark as its icon.
; Written under the installing user's classes so an unprivileged install still
; gets the association, and removed with the app.
[Registry]
Root: HKA; Subkey: "Software\Classes\{#MyAssocExt}"; ValueType: string; ValueData: "{#MyAssocKey}"; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\{#MyAssocKey}"; ValueType: string; ValueData: "Aria project"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#MyAssocKey}\DefaultIcon"; ValueType: string; ValueData: "{app}\{#MyAppIcon}"
Root: HKA; Subkey: "Software\Classes\{#MyAssocKey}\shell\open\command"; ValueType: string; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\{#MyAppExeName}\SupportedTypes"; ValueType: string; ValueName: "{#MyAssocExt}"; ValueData: ""; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
