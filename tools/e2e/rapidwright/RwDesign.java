/*
 * Copyright 2017-2022 F4PGA Authors
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

import com.xilinx.rapidwright.design.Cell;
import com.xilinx.rapidwright.design.Design;
import com.xilinx.rapidwright.design.Net;
import com.xilinx.rapidwright.design.Unisim;
import com.xilinx.rapidwright.device.Site;
import com.xilinx.rapidwright.device.SiteTypeEnum;
import com.xilinx.rapidwright.interchange.LogNetlistWriter;
import com.xilinx.rapidwright.interchange.PhysNetlistWriter;
import com.xilinx.rapidwright.router.Router;

import java.util.ArrayList;
import java.util.Arrays;
import java.util.List;
import java.util.Random;

/**
 * T7.5: a placed and routed 7 series design made by RapidWright alone (no
 * Vivado), written as an FPGA interchange logical and physical netlist for
 * python-fpga-interchange's xc7 FASM generator.
 *
 * <pre>
 * RwDesign PART OUT_PREFIX STAGES SEED
 * </pre>
 *
 * A ring of STAGES register stages on random slices: stage i is a LUT6 (a
 * random INIT) on a random BEL letter whose six inputs come from the
 * flip-flops of six earlier stages, driving a flip-flop (FDRE or FDSE,
 * random INIT) in the same slice. Clock, clock enable and reset are not
 * connected (no clock routing: RapidWright's RWRoute does not route 7
 * series parts, its older {@code router.Router} does, without clocks).
 * Writes OUT_PREFIX.netlist and OUT_PREFIX.phys.
 */
public class RwDesign {

    public static void main(String[] args) throws Exception {
        if (args.length != 4) {
            System.err.println("usage: RwDesign PART OUT_PREFIX STAGES SEED");
            System.exit(2);
        }
        String part = args[0];
        int stages = Integer.parseInt(args[2]);
        Random rnd = new Random(Long.parseLong(args[3]));
        Design d = new Design("top", part);
        List<Site> slices = new ArrayList<>();
        slices.addAll(Arrays.asList(d.getDevice().getAllSitesOfType(SiteTypeEnum.SLICEL)));
        slices.addAll(Arrays.asList(d.getDevice().getAllSitesOfType(SiteTypeEnum.SLICEM)));
        slices.sort((a, b) -> a.getName().compareTo(b.getName()));
        String[] letters = {"A", "B", "C", "D"};
        List<Cell> luts = new ArrayList<>();
        List<Cell> ffs = new ArrayList<>();
        List<String> used = new ArrayList<>();
        for (int i = 0; i < stages; i++) {
            String loc;
            do {
                Site s = slices.get(rnd.nextInt(slices.size()));
                loc = s.getName() + "/" + letters[rnd.nextInt(4)];
            } while (used.contains(loc));
            used.add(loc);
            Cell lut = d.createAndPlaceCell("lut" + i, Unisim.LUT6, loc + "6LUT");
            lut.addProperty("INIT", String.format("64'h%016X", rnd.nextLong()));
            boolean set = rnd.nextBoolean();
            Cell ff = d.createAndPlaceCell("ff" + i, set ? Unisim.FDSE : Unisim.FDRE, loc + "FF");
            ff.addProperty("INIT", rnd.nextBoolean() ? "1'b1" : "1'b0");
            Net n = d.createNet("d" + i);
            n.connect(lut, "O");
            n.connect(ff, "D");
            luts.add(lut);
            ffs.add(ff);
        }
        for (int i = 0; i < stages; i++) {
            Net q = d.createNet("q" + i);
            q.connect(ffs.get(i), "Q");
        }
        for (int i = 0; i < stages; i++) {
            for (int k = 0; k < 6; k++) {
                int src = Math.floorMod(i - 1 - k * 3, stages);
                d.getNet("q" + src).connect(luts.get(i), "I" + k);
            }
        }
        d.routeSites();
        new Router(d).routeDesign();
        LogNetlistWriter.writeLogNetlist(d.getNetlist(), args[1] + ".netlist");
        PhysNetlistWriter.writePhysNetlist(d, args[1] + ".phys");
        int pips = 0;
        int unrouted = 0;
        for (Net n : d.getNets()) {
            pips += n.getPIPs().size();
            if (n.getName().startsWith("q") && n.getPIPs().isEmpty()) {
                unrouted++;
            }
        }
        System.out.println("RwDesign: " + stages + " stages, " + pips + " PIPs, "
                + unrouted + " inter-site nets without PIPs");
    }
}
