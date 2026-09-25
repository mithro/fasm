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

import com.xilinx.rapidwright.bitstream.Bitstream;
import com.xilinx.rapidwright.bitstream.BitstreamHeader;
import com.xilinx.rapidwright.bitstream.Block;
import com.xilinx.rapidwright.bitstream.ConfigArray;
import com.xilinx.rapidwright.bitstream.ConfigRow;
import com.xilinx.rapidwright.bitstream.FAR;
import com.xilinx.rapidwright.bitstream.Frame;
import com.xilinx.rapidwright.bitstream.IDCode;
import com.xilinx.rapidwright.bitstream.Packet;
import com.xilinx.rapidwright.device.Device;
import com.xilinx.rapidwright.device.Series;

import java.io.BufferedReader;
import java.io.BufferedWriter;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.List;
import java.util.TreeMap;

/**
 * T7.5 driver: dumps what RapidWright's (closed source, public API)
 * {@code com.xilinx.rapidwright.bitstream} package knows about a part's
 * configuration array and about a {@code .bit} file, in formats the Python
 * side ({@code tools/e2e/rapidwright/rwcheck.py}) compares with the Rust
 * tools.
 *
 * <pre>
 * batch FILE                    the commands below, one per line, tab separated
 * layout PART OUT.json          configuration array of PART
 * read BIT OUT.frm OUT.json     every frame of the walk (.frm, ECC kept)
 *                               and the header / packet list
 * write PART FRM OUT.bit        a RapidWright bitstream holding FRM's frames
 * rewrite BIT OUT.bit           read BIT and write it back
 * </pre>
 */
public class RwCheck {

    public static void main(String[] args) throws Exception {
        if (args.length == 2 && args[0].equals("batch")) {
            // One command per line, arguments separated by tabs; a failing
            // command prints "FAIL <line number> <exception>" and the batch
            // goes on (one JVM and one device load for many commands).
            List<String> lines = Files.readAllLines(Paths.get(args[1]), StandardCharsets.UTF_8);
            int n = 0;
            for (String line : lines) {
                n++;
                if (line.isBlank()) {
                    continue;
                }
                try {
                    run(line.split("\t"));
                    System.out.println("OK " + n);
                } catch (Throwable t) {
                    System.out.println("FAIL " + n + " " + String.valueOf(t).replace('\n', ' '));
                }
                System.out.flush();
            }
        } else if (!run(args)) {
            System.err.println("usage: RwCheck batch FILE | layout PART OUT.json"
                    + " | read BIT OUT.frm OUT.json | write PART FRM OUT.bit | rewrite BIT OUT.bit");
            System.exit(2);
        }
    }

    static boolean run(String[] args) throws Exception {
        if (args.length == 3 && args[0].equals("layout")) {
            layout(args[1], Paths.get(args[2]));
        } else if (args.length == 4 && args[0].equals("read")) {
            read(Paths.get(args[1]), Paths.get(args[2]), Paths.get(args[3]));
        } else if (args.length == 4 && args[0].equals("write")) {
            write(args[1], Paths.get(args[2]), Paths.get(args[3]));
        } else if (args.length == 3 && args[0].equals("rewrite")) {
            Bitstream b = Bitstream.readBitstream(Paths.get(args[1]));
            if (!b.writeBitstream(Paths.get(args[2]))) {
                throw new IOException("writeBitstream failed");
            }
        } else {
            return false;
        }
        return true;
    }

    static String hex(int v) {
        return String.format("0x%08X", v);
    }

    static String q(String s) {
        StringBuilder sb = new StringBuilder("\"");
        for (char c : (s == null ? "" : s).toCharArray()) {
            if (c == '"' || c == '\\') {
                sb.append('\\').append(c);
            } else if (c < 0x20) {
                sb.append(String.format("\\u%04x", (int) c));
            } else {
                sb.append(c);
            }
        }
        return sb.append('"').toString();
    }

    /** The frame addresses of the part in FAR increment order, from 0. */
    static List<Integer> walk(ConfigArray ca) {
        List<Integer> out = new ArrayList<>();
        FAR far = new FAR(ca);
        far.setFAR(0);
        int first = far.getCurrentFAR();
        out.add(first);
        while (true) {
            int next = far.incrementFAR();
            if (next == -1 || far.getCurrentFAR() == first) {
                break;
            }
            out.add(far.getCurrentFAR());
            if (out.size() > 10_000_000) {
                throw new IllegalStateException("FAR walk does not end");
            }
        }
        return out;
    }

    static void layout(String part, Path out) throws IOException {
        Bitstream.checkIfDeviceSupported(part);
        Device dev = Device.getDevice(part);
        Series series = dev.getSeries();
        ConfigArray ca = new ConfigArray(dev);
        StringBuilder j = new StringBuilder();
        j.append("{\n");
        j.append("  \"part\": ").append(q(part)).append(",\n");
        j.append("  \"device\": ").append(q(dev.getName())).append(",\n");
        j.append("  \"series\": ").append(q(series.name())).append(",\n");
        Integer id = IDCode.getIDCode(dev);
        j.append("  \"idcode\": ").append(id == null ? "null" : q(hex(id))).append(",\n");
        j.append("  \"words_per_frame\": ").append(Frame.getWordsPerFrame(series)).append(",\n");
        j.append("  \"frame_overhead_count_per_row\": ")
                .append(ConfigArray.FRAME_OVERHEAD_COUNT_PER_ROW).append(",\n");
        j.append("  \"config_array_words\": ").append(ca.getWordSize()).append(",\n");
        // Columns keyed by (block type, top/bottom, row) like part.yaml.
        j.append("  \"columns\": [\n");
        boolean firstCol = true;
        for (ConfigRow r : ca.getConfigRows()) {
            for (Block b : r.getBlocks()) {
                int a = b.getAddress();
                if (!firstCol) {
                    j.append(",\n");
                }
                firstCol = false;
                j.append("    {\"far\": ").append(q(hex(a)))
                        .append(", \"block_type\": ").append(FAR.getBlockType(a, series))
                        .append(", \"top_bottom\": ").append(FAR.getTopBotBit(a, series))
                        .append(", \"row\": ").append(FAR.getRowAddress(a, series))
                        .append(", \"column\": ").append(FAR.getColumnAddress(a, series))
                        .append(", \"frames\": ").append(b.getFrameCount())
                        .append(", \"subtype\": ").append(q(subtype(b)))
                        .append(", \"tile_column\": ").append(b.getTileColumn())
                        .append(", \"config_row\": ").append(r.getRowIndex())
                        .append("}");
            }
        }
        j.append("\n  ],\n");
        List<Integer> w = walk(ca);
        j.append("  \"walk_frames\": ").append(w.size()).append(",\n");
        j.append("  \"walk_sha256\": ").append(q(sha256Walk(w))).append("\n");
        j.append("}\n");
        Files.write(out, j.toString().getBytes(StandardCharsets.UTF_8));
    }

    /** The block's sub type name ("?" where RapidWright has no tile type). */
    static String subtype(Block b) {
        try {
            return String.valueOf(b.getSubType());
        } catch (RuntimeException e) {
            return "?";
        }
    }

    /** SHA-256 of the walk as lines "0x%08X\n" (what rwcheck.py hashes too). */
    static String sha256Walk(List<Integer> w) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-256");
            for (int a : w) {
                md.update((hex(a) + "\n").getBytes(StandardCharsets.US_ASCII));
            }
            StringBuilder sb = new StringBuilder();
            for (byte b : md.digest()) {
                sb.append(String.format("%02x", b));
            }
            return sb.toString();
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    static void read(Path bit, Path frm, Path json) throws IOException {
        Bitstream b = Bitstream.readBitstream(bit);
        // getConfigArray() alone gives an array without the FDRI data:
        // configureArray() fills it from the packets.
        ConfigArray ca = b.configureArray();
        List<Integer> w = walk(ca);
        try (BufferedWriter out = Files.newBufferedWriter(frm, StandardCharsets.US_ASCII)) {
            for (int a : w) {
                FAR far = new FAR(ca);
                far.setFAR(a);
                Frame f = ca.getFrame(far);
                out.write(hex(a));
                out.write(' ');
                int[] words = f.getWords();
                for (int i = 0; i < words.length; i++) {
                    if (i > 0) {
                        out.write(',');
                    }
                    out.write(hex(words[i]));
                }
                out.write('\n');
            }
        }
        BitstreamHeader h = b.getHeader();
        StringBuilder j = new StringBuilder("{\n");
        j.append("  \"design_name\": ").append(q(h == null ? null : h.getDesignName())).append(",\n");
        j.append("  \"options\": ").append(q(h == null ? null : h.getOptions())).append(",\n");
        j.append("  \"part_name\": ").append(q(h == null ? null : h.getPartName())).append(",\n");
        j.append("  \"date\": ").append(q(h == null ? null : h.getDate())).append(",\n");
        j.append("  \"time\": ").append(q(h == null ? null : h.getTime())).append(",\n");
        j.append("  \"device\": ").append(q(b.getDevice().getName())).append(",\n");
        j.append("  \"series\": ").append(q(b.getSeries().name())).append(",\n");
        j.append("  \"walk_frames\": ").append(w.size()).append(",\n");
        j.append("  \"packets\": [\n");
        List<Packet> ps = b.getPackets();
        for (int i = 0; i < ps.size(); i++) {
            Packet p = ps.get(i);
            int[] d = p.getData();
            j.append("    [").append(q(hex(p.getHeader()))).append(", ")
                    .append(d == null ? 0 : d.length).append(", ")
                    .append(q(d == null ? "" : sha256Words(d))).append(", ")
                    .append(q(String.valueOf(p.getRegister()))).append("]")
                    .append(i + 1 < ps.size() ? ",\n" : "\n");
        }
        j.append("  ]\n}\n");
        Files.write(json, j.toString().getBytes(StandardCharsets.UTF_8));
    }

    /** SHA-256 of the words, big endian (as in the .bit). */
    static String sha256Words(int[] d) {
        try {
            MessageDigest md = MessageDigest.getInstance("SHA-256");
            byte[] buf = new byte[4];
            for (int v : d) {
                buf[0] = (byte) (v >>> 24);
                buf[1] = (byte) (v >>> 16);
                buf[2] = (byte) (v >>> 8);
                buf[3] = (byte) v;
                md.update(buf);
            }
            StringBuilder sb = new StringBuilder();
            for (byte x : md.digest()) {
                sb.append(String.format("%02x", x));
            }
            return sb.toString();
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    static void write(String part, Path frm, Path out) throws IOException {
        Device dev = Device.getDevice(part);
        TreeMap<Integer, int[]> frames = new TreeMap<>();
        try (BufferedReader r = Files.newBufferedReader(frm, StandardCharsets.US_ASCII)) {
            String line;
            while ((line = r.readLine()) != null) {
                line = line.trim();
                if (line.isEmpty() || line.startsWith("#")) {
                    continue;
                }
                String[] parts = line.split(" ", 2);
                int addr = (int) Long.parseLong(parts[0].substring(2), 16);
                String[] ws = parts[1].split(",");
                int[] words = new int[ws.length];
                for (int i = 0; i < ws.length; i++) {
                    words[i] = (int) Long.parseLong(ws[i].trim().substring(2), 16);
                }
                frames.put(addr, words);
            }
        }
        Bitstream b = new Bitstream("rwcheck", part);
        ConfigArray ca = b.configureArray();
        for (var e : frames.entrySet()) {
            FAR far = new FAR(ca);
            if (!far.setFAR(e.getKey())) {
                throw new IllegalArgumentException("frame " + hex(e.getKey()) + " not in " + part);
            }
            Frame f = ca.getFrame(far);
            f.setWords(e.getValue().clone());
            f.updateECCBits();
        }
        b.updatePacketsFromConfigArray();
        if (!b.writeBitstream(out)) {
            throw new IOException("writeBitstream failed");
        }
    }
}
