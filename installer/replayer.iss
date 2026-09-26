#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceDir
  #error SourceDir is required
#endif
[Setup]
AppId={{18561AE2-CC3B-47E7-9468-1255D7569D86}
AppName=replayer
AppVersion={#AppVersion}
AppPublisher=L-Chris
AppPublisherURL=https://github.com/L-Chris/replayer
DefaultDirName={localappdata}\Programs\replayer
DefaultGroupName=replayer
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=replayer-{#AppVersion}-windows-x86_64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#SourceDir}\replayer.ico
UninstallDisplayIcon={app}\replayer.exe
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\replayer"; Filename: "{app}\replayer.exe"; IconFilename: "{app}\replayer.exe"; IconIndex: 0
Name: "{autodesktop}\replayer"; Filename: "{app}\replayer.exe"; IconFilename: "{app}\replayer.exe"; IconIndex: 0; Tasks: desktopicon

[Run]
Filename: "{app}\replayer.exe"; Description: "Launch replayer"; Flags: nowait postinstall skipifsilent
