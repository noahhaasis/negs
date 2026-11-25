$r = 'C:\Users\Noah\.negs\scratch\gpui-look'
& "$r\load.ps1"
$win = 'C:\Users\Noah\Projects\negs\windows\target\release\Negs.exe'
$i = 0
foreach ($k in @('gpui', 'win32', 'win32', 'gpui', 'gpui', 'win32', 'win32', 'gpui')) {
  $i++
  $exe = if ($k -eq 'gpui') { "$r\bin-look\Negs.exe" } else { $win }
  & "$r\run.ps1" -Exe $exe -Photo "$r\photos-$k\DSC_0262.NEF" -Profile "$r\prof-$k" -Out "$r\out\abba-$i-$k" -Script "$r\scen-$k.txt" -Settle 6 -Theme darkroom
  "run $i $k done"
}
& "$r\load.ps1"
& "$r\startup.ps1" -A "$r\bin-look\Negs.exe" -B $win -Runs 8 -Theme darkroom
& "$r\load.ps1"
