package main

import (
	"bufio"
	"fmt"
	"os"
	"strings"

	"github.com/oleksandr/fbp"
)

func exportNetworkGraph(nw *fbp.Fbp) error {
	out := bufio.NewWriter(os.Stdout)

	out.WriteString("digraph {\n" +
		"\tnodesep=1.2;\n" +
		//"\tranksep=1;\n" +
		"\tconcentrate=true;\n" +
		"\tedge [labelfontname=\"DejaVu Sans Condensed\",labelfontsize=10,labeldistance=1.5];\n" +
		"\tnode [fontname=\"DejaVu Sans Condensed\",fontsize=14];\n" +
		"\tgraph [fontname=\"DejaVu Sans Condensed-Bold\",fontsize=18];\n" +
		"\n" +
		"\tsubgraph cluster_netin {\n" +
		"\t\tlabel=\"Imported Ports\";\n")
	// network in ports
	for netinPort := range nw.Inports {
		out.WriteString(fmt.Sprintf("\t\t%s [shape=rarrow];\n", netinPort))
	}
	out.WriteString("\t}\n" +
		"\n" +
		"\tsubgraph cluster_netout {\n" +
		"\t\tlabel=\"Exported Ports\";\n")
	// network out ports
	for netoutPort := range nw.Outports {
		out.WriteString(fmt.Sprintf("\t\t%s [shape=larrow];\n", netoutPort))
	}
	out.WriteString("\t}\n" +
		"\n" +
		"\tsubgraph cluster_netmain {\n" +
		"\t\tlabel=\"Network\";" +
		"\t\tmargin=25;\n")
	// all nodes/vertices = processes, netin ports, netout ports and IIPs  //TODO
	for netinPort := range nw.Inports {
		out.WriteString(fmt.Sprintf("\t\t%s;\n", netinPort))
	}
	for index, conn := range nw.Connections {
		if conn.Source == nil {
			// is an IIP, use index as name
			//TODO maybe start own IIP index number
			// NOTE: Replace() is a simple one-level escape of " with literial \"
			out.WriteString(fmt.Sprintf("\t\tIIP%d [label=\"'%s'\",shape=note];\n", index, strings.Replace(conn.Data, "\"", "\\\"", -1)))
		}
	}
	for _, process := range nw.Processes {
		out.WriteString(fmt.Sprintf("\t\t%s [shape=component,style=rounded];\n", process.Name))
	}
	for netoutPort := range nw.Outports {
		out.WriteString(fmt.Sprintf("\t\t%s;\n", netoutPort))
	}
	out.WriteString("\t}\n" +
		"\n")
	// all edges = connections from netin ports, to netout ports, between processen and from IIPs
	for netinPort, conn := range nw.Inports {
		out.WriteString(fmt.Sprintf("\t%s -> %s [headlabel=\"%s\"];\n", netinPort, conn.Process, conn.Port))
	}
	for index, conn := range nw.Connections {
		toProcess := conn.Target.Process
		toPort := conn.Target.Port
		if conn.Source == nil {
			// connection from IIP to process
			fromProcess := fmt.Sprintf("IIP%d", index)
			out.WriteString(fmt.Sprintf("\t%s -> %s [shape=rect,headlabel=\"%s\"];\n", fromProcess, toProcess, toPort))
		} else {
			// regular connection between processes
			fromProcess := conn.Source.Process
			fromPort := conn.Source.Port
			out.WriteString(fmt.Sprintf("\t%s -> %s [taillabel=\"%s\",headlabel=\"%s\"];\n", fromProcess, toProcess, fromPort, toPort))
		}
	}
	for netoutPort, conn := range nw.Outports {
		out.WriteString(fmt.Sprintf("\t%s -> %s [taillabel=\"%s\"];\n", conn.Process, netoutPort, conn.Port))
	}
	out.WriteString("}")

	_ = out.Flush()
	return nil
}
