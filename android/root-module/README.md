# RemoteEnvCollector Root Guard

This is a common Magisk/KernelSU module for the Android collector. It does not
contain the APK; install RemoteEnvCollector first, then flash the module ZIP.

The module starts after Android finishes booting and every 30 seconds checks the
collector process, foreground service, accessibility service registration, Doze
exemption, background AppOps, runtime permissions and `oom_score_adj`. It starts
the foreground service directly and never launches the application activity.
Relaunches are limited to once per minute to avoid a crash loop.

Build the flashable archive with `build-module.ps1`. Root permission grants are
best effort because permissions absent on an Android release are ignored.
