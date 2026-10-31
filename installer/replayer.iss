#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef AppIconFile
  #error AppIconFile is required
#endif
[Setup]
AppId={{18561AE2-CC3B-47E7-9468-1255D7569D86}
AppName=replayer
AppVersion={#AppVersion}
AppPublisher=L-Chris
AppPublisherURL=https://github.com/PordoTech/pordo
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
SetupIconFile={#SourceDir}\icons\{#AppIconFile}
UninstallDisplayIcon={app}\icons\{#AppIconFile}
CloseApplications=yes
RestartApplications=no
SetupLogging=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; System media components are now statically registered in replayer.exe.
Type: files; Name: "{app}\plugins\native.json"
Type: files; Name: "{app}\plugins\remdia_backend_native.dll"
Type: files; Name: "{app}\plugins\windows.json"
Type: files; Name: "{app}\plugins\remdia_backend_windows.dll"
Type: files; Name: "{app}\plugins\image.json"
Type: files; Name: "{app}\plugins\remdia_backend_image.dll"
Type: files; Name: "{app}\plugins\system_audio.json"
Type: files; Name: "{app}\plugins\remdia_backend_system_audio.dll"
Type: files; Name: "{app}\plugins\system_demux.json"
Type: files; Name: "{app}\plugins\remdia_backend_system_demux.dll"
Type: files; Name: "{app}\plugins\fallback_demux.json"
Type: files; Name: "{app}\plugins\remdia_backend_fallback_demux.dll"
Type: files; Name: "{app}\plugins\sdk.json"
Type: files; Name: "{app}\plugins\remdia_sdk.dll"
; Remove the retired third-party WebP pack from earlier test installations.
Type: files; Name: "{app}\plugins\image_webp.json"
Type: files; Name: "{app}\plugins\remdia_backend_image_webp.dll"
Type: files; Name: "{app}\remdia-licenses\image-webp-MIT.txt"
Type: files; Name: "{app}\remdia-licenses\image-webp-Apache-2.0.txt"
; Windows audio decoding/MP3 encoding now use installed system codecs.
Type: files; Name: "{app}\plugins\fallback_vorbis.json"
Type: files; Name: "{app}\plugins\remdia_backend_fallback_vorbis.dll"
Type: files; Name: "{app}\plugins\fallback_mp3_lame.json"
Type: files; Name: "{app}\plugins\remdia_backend_fallback_mp3_lame.dll"
Type: files; Name: "{app}\remdia-licenses\LAME-COPYING.txt"
Type: files; Name: "{app}\remdia-licenses\mp3lame-encoder-LGPL-3.0.txt"
; Remove obsolete FFmpeg runtime libraries when upgrading to remdia.
Type: files; Name: "{app}\avdevice-63.dll"
Type: files; Name: "{app}\avfilter-12.dll"
Type: files; Name: "{app}\avcodec-63.dll"
Type: files; Name: "{app}\avformat-63.dll"
Type: files; Name: "{app}\avutil-61.dll"
Type: files; Name: "{app}\swresample-7.dll"
Type: files; Name: "{app}\swscale-10.dll"

[Icons]
Name: "{group}\replayer"; Filename: "{app}\replayer.exe"; IconFilename: "{app}\icons\{#AppIconFile}"; IconIndex: 0
Name: "{autodesktop}\replayer"; Filename: "{app}\replayer.exe"; IconFilename: "{app}\icons\{#AppIconFile}"; IconIndex: 0; Check: ShouldUpdateDesktopShortcut

[Run]
Filename: "{app}\replayer.exe"; Description: "Launch replayer"; Flags: nowait postinstall skipifsilent

[Code]
const
  SHCNE_UPDATEITEM = $00002000;
  SHCNF_PATHW_FLUSHNOWAIT = $00002005;

procedure SHChangeNotify(EventID: Integer; Flags: Cardinal;
  const ItemPath: String; Unused: Integer);
  external 'SHChangeNotify@shell32.dll stdcall';

function ShouldUpdateDesktopShortcut: Boolean;
begin
  Result := WizardIsTaskSelected('desktopicon') or
    FileExists(ExpandConstant('{autodesktop}\replayer.lnk'));
end;

procedure NotifyIconChanged(const FileName: String);
begin
  if FileExists(FileName) then
    SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW_FLUSHNOWAIT, FileName, 0);
end;

procedure RefreshPinnedIcons;
var
  Shell, Shortcut: Variant;
  FindRec: TFindRec;
  PinnedDir, LinkPath, TargetPath, IconPath: String;
begin
  { Windows keeps its own copies of pinned links. Update only this
    installation's icon, preserving the pin, target, arguments and AppID. }
  PinnedDir := ExpandConstant(
    '{userappdata}\Microsoft\Internet Explorer\Quick Launch\User Pinned\TaskBar');
  IconPath := ExpandConstant('{app}\icons\{#AppIconFile}');
  if not FindFirst(PinnedDir + '\*.lnk', FindRec) then
    Exit;
  try
    Shell := CreateOleObject('WScript.Shell');
    repeat
      if (FindRec.Attributes and FILE_ATTRIBUTE_DIRECTORY) = 0 then
      begin
        LinkPath := PinnedDir + '\' + FindRec.Name;
        try
          Shortcut := Shell.CreateShortcut(LinkPath);
          TargetPath := Shell.ExpandEnvironmentStrings(Shortcut.TargetPath);
          if CompareText(TargetPath, ExpandConstant('{app}\replayer.exe')) = 0 then
          begin
            Shortcut.IconLocation := '"' + IconPath + '",0';
            Shortcut.Save;
            NotifyIconChanged(LinkPath);
            Log('Updated pinned replayer icon: ' + LinkPath);
          end;
        except
          { A locked/custom link must not prevent the application update. }
          Log('Could not update pinned icon: ' + LinkPath + ': ' + GetExceptionMessage);
        end;
      end;
    until not FindNext(FindRec);
  finally
    FindClose(FindRec);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    NotifyIconChanged(ExpandConstant('{app}\replayer.exe'));
    NotifyIconChanged(ExpandConstant('{app}\icons\{#AppIconFile}'));
    NotifyIconChanged(ExpandConstant('{group}\replayer.lnk'));
    NotifyIconChanged(ExpandConstant('{autodesktop}\replayer.lnk'));
    try
      RefreshPinnedIcons;
    except
      Log('Could not refresh pinned icons: ' + GetExceptionMessage);
    end;
  end;
end;
