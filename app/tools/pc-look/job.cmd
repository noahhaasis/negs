@echo off
del /q C:\Users\Noah\.negs\scratch\gpui-look\job.done 2>nul
powershell -NoProfile -ExecutionPolicy Bypass -File C:\Users\Noah\.negs\scratch\gpui-look\job.ps1 > C:\Users\Noah\.negs\scratch\gpui-look\job.out 2>&1
echo done > C:\Users\Noah\.negs\scratch\gpui-look\job.done
