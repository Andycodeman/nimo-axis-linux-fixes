#!/bin/sh
# Compile wmcc-fixed.asl and embed it in the driver as ../wmcc-fixed.h. Needs acpica-tools.
set -e
cd "$(dirname "$0")"
iasl -p wmcc-fixed wmcc-fixed.asl
{
	echo '/* Generated from aml/wmcc-fixed.asl by aml/build.sh. Do not edit. */'
	xxd -i -n nimo_npwr_ssdt wmcc-fixed.aml |
		sed 's/^unsigned char/static const unsigned char/; s/^unsigned int/static const unsigned int/'
} > ../wmcc-fixed.h
rm -f wmcc-fixed.aml
