; RENotepad installer (Inno Setup 7). Build it with scripts\package.ps1, which
; compiles the release exe first and passes /DAppVersion.
;
; Installs for the current user by default (no admin prompt); the first page
; offers "all users" too. Registers RENotepad with Windows so it shows up in
; "Open with" and in Settings > Default apps for text files. Windows doesn't let
; an installer make itself the default, so the last page offers to open the
; Default apps page instead.

#define AppName "RENotepad"
#define AppExe "RENotepad.exe"
#define AppPublisher "Nikko Gonzales"
#define AppURL "https://github.com/nikkoxgonzales/RENotepad"
#define ExePath AddBackslash(SourcePath) + "..\target\release\" + AppExe
#ifndef AppVersion
  #define AppVersion GetStringFileInfo(ExePath, "ProductVersion")
#endif
; One ProgID for every text type: "Text Document" with the system text icon.
#define ProgId "RENotepad.txt"
#define CapKey "Software\Nikko Gonzales\RENotepad\Capabilities"

[Setup]
AppId={{9562D408-3BDB-4707-8F3C-5AD893972561}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
AppCopyright=© 2026 {#AppPublisher}. MIT licensed.
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\RENotepad.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern dynamic
ChangesAssociations=yes
CloseApplications=yes
Compression=lzma2/max
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename={#AppName}-{#AppVersion}-setup

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: editwith; Description: "Add ""Edit with RENotepad"" to the right-click menu of every file"; GroupDescription: "Shell:"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

; Per extension: listed under "Open with", a type the app says it handles, and a
; choice on the Default apps page.
; Keys that didn't exist before are removed again on uninstall if left empty.
#define Ext(E) \
  "Root: HKA; Subkey: ""Software\Classes\." + E + """; Flags: uninsdeletekeyifempty" + NewLine + \
  "Root: HKA; Subkey: ""Software\Classes\." + E + "\OpenWithProgids""; ValueType: string; ValueName: """ + ProgId + """; ValueData: """"; Flags: uninsdeletevalue uninsdeletekeyifempty" + NewLine + \
  "Root: HKA; Subkey: ""Software\Classes\Applications\" + AppExe + "\SupportedTypes""; ValueType: string; ValueName: ""." + E + """; ValueData: """"" + NewLine + \
  "Root: HKA; Subkey: """ + CapKey + "\FileAssociations""; ValueType: string; ValueName: ""." + E + """; ValueData: """ + ProgId + """" + NewLine

[Registry]
; "renotepad" from Run (Win+R) and the command line.
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#AppExe}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe}"; Flags: uninsdeletekey
; The file type RENotepad opens.
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "Text Document"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: expandsz; ValueName: ""; ValueData: "%SystemRoot%\System32\imageres.dll,-102"
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
; The app itself, for "Open with" > "Choose another app".
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
; Settings > Default apps.
Root: HKA; Subkey: "Software\Nikko Gonzales"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Nikko Gonzales\RENotepad"; Flags: uninsdeletekey
Root: HKA; Subkey: "{#CapKey}"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"
Root: HKA; Subkey: "{#CapKey}"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "{#CapKey}"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "Windows 10's Notepad, reverse-engineered and rebuilt in Rust, with the Windows 11 look."
Root: HKA; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppName}"; ValueData: "{#CapKey}"; Flags: uninsdeletevalue
{#Ext("txt")}
{#Ext("log")}
{#Ext("ini")}
{#Ext("cfg")}
{#Ext("conf")}
{#Ext("md")}
{#Ext("csv")}
{#Ext("tsv")}
{#Ext("json")}
{#Ext("xml")}
{#Ext("yaml")}
{#Ext("yml")}
{#Ext("toml")}
{#Ext("nfo")}
{#Ext("srt")}
; Optional "Edit with RENotepad" on every file.
Root: HKA; Subkey: "Software\Classes\*\shell\RENotepad"; ValueType: string; ValueName: ""; ValueData: "Edit with {#AppName}"; Flags: uninsdeletekey; Tasks: editwith
Root: HKA; Subkey: "Software\Classes\*\shell\RENotepad"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#AppExe},0"; Tasks: editwith
Root: HKA; Subkey: "Software\Classes\*\shell\RENotepad\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: editwith

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
Filename: "{code:DefaultAppsUri}"; Description: "Choose {#AppName} as the default app for text files (opens Settings)"; Flags: shellexec nowait postinstall skipifsilent unchecked

[Code]
// Windows 11 opens RENotepad's own page; Windows 10 opens the Default apps list.
function DefaultAppsUri(Param: String): String;
begin
  if IsAdminInstallMode then
    Result := 'ms-settings:defaultapps?registeredAppMachine={#AppName}'
  else
    Result := 'ms-settings:defaultapps?registeredAppUser={#AppName}';
end;
