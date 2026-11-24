' Steam Launch Options entry point. Run via wscript.exe (which never allocates
' a console, unlike a .bat file's cmd.exe host) so nothing visible appears
' before or during CS2's launch.
'
' Starts the listener detached and hidden, then reconstructs the real game
' launch command from this script's own arguments (Steam substitutes
' %command% with the actual cs2.exe invocation) and runs it normally.
' Prefers a listener sitting next to this script (the deployed layout that
' deploy_to_windows.sh produces on a local drive), falling back to the build
' output in the repo. Running from a local drive is what keeps Windows from
' showing an "unverified publisher" prompt on every launch: a UNC path like
' \\wsl.localhost\... lands in the Internet zone, which prompts for unsigned
' executables, and the "don't ask again" answer can't be stored on ext4.
Set fso = CreateObject("Scripting.FileSystemObject")
scriptDir = fso.GetParentFolderName(WScript.ScriptFullName)
listenerPath = fso.BuildPath(scriptDir, "gsi-listener.exe")
If Not fso.FileExists(listenerPath) Then
    listenerPath = fso.BuildPath(scriptDir, "..\target\x86_64-pc-windows-gnu\release\gsi-listener.exe")
End If

Set WshShell = CreateObject("WScript.Shell")
WshShell.Run """" & listenerPath & """", 0, False

cmdLine = ""
For i = 0 To WScript.Arguments.Count - 1
    cmdLine = cmdLine & " """ & Replace(WScript.Arguments(i), """", """""") & """"
Next
cmdLine = Trim(cmdLine)
If Len(cmdLine) > 0 Then
    WshShell.Run cmdLine, 1, False
End If

Set WshShell = Nothing
Set fso = Nothing
