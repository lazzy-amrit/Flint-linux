# Linux AppImage packaging

The AppImage must use the host's EGL/GL/GBM, Wayland, X11, XCB, and
xkbcommon libraries.  These libraries are coupled to the host display stack;
bundling them beside a host Mesa `libEGL` can make WebKitGTK abort during its
initial EGL probe.

The supported build environment is `packaging/Dockerfile` (Ubuntu 24.04):

```sh
docker build -f packaging/Dockerfile -t flint-appimage-builder:24.04 .
```

Inside that image, provide `linuxdeploy` and its AppImage output plugin, then
run:

```sh
./packaging/build-appimage.sh
```

The build script excludes host-coupled graphics libraries at deployment time,
removes any that a plugin copied anyway, installs the custom `AppRun`, and
fails if a forbidden display-stack library remains in the AppDir.
