$r = 'C:\Users\Noah\.negs\scratch\gpui-look'
$wt = 'C:\Users\Noah\Projects\negs\worktrees\gpui-look'
$win = 'C:\Users\Noah\Projects\negs\windows\target\release\Negs.exe'
New-Item -ItemType Directory -Force -Path "$r\bin-final", "$r\themes" | Out-Null
$gp = "$r\bin-final\Negs.exe"
& "$r\load.ps1"
foreach ($t in @('darkroom', 'contactsheet', 'lighttable', 'daylight', 'plain', 'plaindark')) {
  & "$r\run.ps1" -Exe $gp -Photo "$r\photos-gpui\DSC_0262.NEF" -Profile "$r\prof-gpui" -Out "$r\themes\$t-gpui" -Shot -Settle 6 -Theme $t
  & "$r\run.ps1" -Exe $win -Photo "$r\photos-win32\DSC_0262.NEF" -Profile "$r\prof-win32" -Out "$r\themes\$t-win32" -Shot -Settle 6 -Theme $t
  "theme $t done"
}
$i = 0
foreach ($k in @('gpui', 'win32', 'win32', 'gpui')) {
  $i++
  $exe = if ($k -eq 'gpui') { $gp } else { $win }
  & "$r\run.ps1" -Exe $exe -Photo "$r\photos-$k\DSC_0262.NEF" -Profile "$r\prof-$k" -Out "$r\out\final-$i-$k" -Script "$r\scen-$k.txt" -Settle 6 -Theme darkroom
  "run $i $k done"
}
& "$r\load.ps1"
