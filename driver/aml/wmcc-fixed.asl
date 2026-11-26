/*
 * A fixed copy of \_SB.EWMI.WMCC (WMI "SetOemPowerSwitch", GUID
 * ABBC0FB8-8EA1-11D1-A000-C90629100000) from BIOS ASH959V2.0_5_FTPM (2026-09-04),
 * loaded by nimo-wmi as a small SSDT under the new name \_SB.EWMI.NPWR.
 * NIMO's own method is left alone. Only two things differ from it:
 *  - CreateField (Local0, 0x20, 0x3F, RES3) is now 0x20 bits long. The original
 *    runs past the end of the 8-byte buffer and Linux rejects the whole method.
 *  - Names are fully qualified.
 *
 * Arg1 = 1: set mode Arg2 (1 Quiet, 2 Balanced, 3 Performance)
 * Arg1 = 2: get mode (returned in bytes 2-3)
 *
 * After editing, run ./build.sh to regenerate ../wmcc-fixed.h.
 */
DefinitionBlock ("", "SSDT", 2, "NIMO", "NIMOPWR", 0x00000001)
{
    External (\_SB.CMSW, MethodObj)
    External (\_SB.PCI0.SBRG.H_EC.ECPM, IntObj)
    External (\_SB.PCI0.SBRG.H_EC.E004, MethodObj)
    External (\_SB.EWMI, DeviceObj)

    Method (\_SB.EWMI.NPWR, 3, Serialized)
    {
        Debug = "nimo-wmi NPWR (fixed WMCC)"
        Local0 = Buffer (0x08){}
        CreateWordField (Local0, Zero, RTNC)
        CreateWordField (Local0, 0x02, WMPM)
        CreateField (Local0, 0x20, 0x20, RES3)   // was 0x3F: 31 bits past the end of the buffer
        RTNC = One
        Switch (ToInteger (Arg1))
        {
            Case (One)
            {
                \_SB.CMSW (0x86, Arg1)
                If ((ToInteger (Arg2) == One))
                {
                    \_SB.CMSW (0x87, Arg2)
                    \_SB.PCI0.SBRG.H_EC.ECPM = Arg2
                    WMPM = Arg2
                    \_SB.PCI0.SBRG.H_EC.E004 ()
                    RTNC = Zero
                    Return (Local0)
                }
    
                If ((ToInteger (Arg2) == 0x02))
                {
                    \_SB.CMSW (0x87, Arg2)
                    \_SB.PCI0.SBRG.H_EC.ECPM = Arg2
                    WMPM = Arg2
                    \_SB.PCI0.SBRG.H_EC.E004 ()
                    RTNC = Zero
                    Return (Local0)
                }
    
                If ((ToInteger (Arg2) == 0x03))
                {
                    \_SB.CMSW (0x87, Arg2)
                    \_SB.PCI0.SBRG.H_EC.ECPM = Arg2
                    WMPM = Arg2
                    \_SB.PCI0.SBRG.H_EC.E004 ()
                    RTNC = Zero
                    Return (Local0)
                }
            }
            Case (0x02)
            {
                WMPM = \_SB.PCI0.SBRG.H_EC.ECPM
                RTNC = Zero
                Return (Local0)
            }
    
        }
    
        Return (Local0)
    }
}
