#!/bin/sh
exec xinit /usr/lib/yargos/run-yarg -- :0 vt1 -nolisten tcp -novtswitch -keeptty
