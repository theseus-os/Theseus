from statistics import mean 
from statistics import stdev 
from prettytable import PrettyTable

m100_mp_map = []
m100_vma_map = []

m100_mp_remap = []
m100_vma_remap = []

m100_mp_unmap = []
m100_vma_unmap = []

m1000_mp_map = []
m1000_vma_map = []

m1000_mp_remap = []
m1000_vma_remap = []

m1000_mp_unmap = []
m1000_vma_unmap = []

m10000_mp_map = []
m10000_vma_map = []

m10000_mp_remap = []
m10000_vma_remap = []

m10000_mp_unmap = []
m10000_vma_unmap = []

m100000_mp_map = []
m100000_vma_map = []

m100000_mp_remap = []
m100000_vma_remap = []

m100000_mp_unmap = []
m100000_vma_unmap = []

for i in range(0,10):
    file_name = "".join(["./results/results_", str(i), ".txt"])
    for line in open(file_name,'r'):
        if 'mapped_pages' in line:
            tokens = line.split()
            if '100' in tokens:
                m100_mp_map.append(float(tokens[4]))
                m100_mp_remap.append(float(tokens[6]))
                m100_mp_unmap.append(float(tokens[8]))
            if '1000' in tokens:
                m1000_mp_map.append(float(tokens[4]))
                m1000_mp_remap.append(float(tokens[6]))
                m1000_mp_unmap.append(float(tokens[8]))
            if '10000' in tokens:
                m10000_mp_map.append(float(tokens[4]))
                m10000_mp_remap.append(float(tokens[6]))
                m10000_mp_unmap.append(float(tokens[8]))
            if '100000' in tokens:
                m100000_mp_map.append(float(tokens[4]))
                m100000_mp_remap.append(float(tokens[6]))
                m100000_mp_unmap.append(float(tokens[8]))

        if 'vmas' in line:
            tokens = line.split()
            if '100' in tokens:
                m100_vma_map.append(float(tokens[4]))
                m100_vma_remap.append(float(tokens[6]))
                m100_vma_unmap.append(float(tokens[8]))
            if '1000' in tokens:
                m1000_vma_map.append(float(tokens[4]))
                m1000_vma_remap.append(float(tokens[6]))
                m1000_vma_unmap.append(float(tokens[8]))
            if '10000' in tokens:
                m10000_vma_map.append(float(tokens[4]))
                m10000_vma_remap.append(float(tokens[6]))
                m10000_vma_unmap.append(float(tokens[8]))
            if '100000' in tokens:
                m100000_vma_map.append(float(tokens[4]))
                m100000_vma_remap.append(float(tokens[6]))
                m100000_vma_unmap.append(float(tokens[8]))


print("Memory mapping benchmark results, corresponding to Figure 3. All results are in nanoseconds.")
print()
table1 = PrettyTable()
table1.field_names = ["Mapping Type","Total Mappings","Map Mean (ns)","Map Std Dev (ns)"]
table1.add_row(["Mapped Pages", 100, mean(m100_mp_map), stdev(m100_mp_map)])
table1.add_row(["VMAs", 100, mean(m100_vma_map), stdev(m100_vma_map)])
table1.add_row(["Mapped Pages", 1000, mean(m1000_mp_map), stdev(m1000_mp_map)])
table1.add_row(["VMAs", 1000, mean(m1000_vma_map), stdev(m1000_vma_map)])
table1.add_row(["Mapped Pages", 10000, mean(m10000_mp_map), stdev(m10000_mp_map)])
table1.add_row(["VMAs", 10000, mean(m10000_vma_map), stdev(m10000_vma_map)])
table1.add_row(["Mapped Pages", 100000, mean(m100000_mp_map), stdev(m100000_mp_map)])
table1.add_row(["VMAs", 100000, mean(m100000_vma_map), stdev(m100000_vma_map)])

print(table1)

print()
table2 = PrettyTable()
table2.field_names = ["Mapping Type","Total Mappings","Remap Mean (ns)","Remap Std Dev (ns)"]
table2.add_row(["Mapped Pages", 100, mean(m100_mp_remap), stdev(m100_mp_remap)])
table2.add_row(["VMAs", 100, mean(m100_vma_remap), stdev(m100_vma_remap)])
table2.add_row(["Mapped Pages", 1000, mean(m1000_mp_remap), stdev(m1000_mp_remap)])
table2.add_row(["VMAs", 1000, mean(m1000_vma_remap), stdev(m1000_vma_remap)])
table2.add_row(["Mapped Pages", 10000, mean(m10000_mp_remap), stdev(m10000_mp_remap)])
table2.add_row(["VMAs", 10000, mean(m10000_vma_remap), stdev(m10000_vma_remap)])
table2.add_row(["Mapped Pages", 100000, mean(m100000_mp_remap), stdev(m100000_mp_remap)])
table2.add_row(["VMAs", 100000, mean(m100000_vma_remap), stdev(m100000_vma_remap)])

print(table2)

print()
table3 = PrettyTable()
table3.field_names = ["Mapping Type","Total Mappings","Unmap Mean (ns)","Unmap Std Dev (ns)"]
table3.add_row(["Mapped Pages", 100, mean(m100_mp_unmap), stdev(m100_mp_unmap)])
table3.add_row(["VMAs", 100, mean(m100_vma_unmap), stdev(m100_vma_unmap)])
table3.add_row(["Mapped Pages", 1000, mean(m1000_mp_unmap), stdev(m1000_mp_unmap)])
table3.add_row(["VMAs", 1000, mean(m1000_vma_unmap), stdev(m1000_vma_unmap)])
table3.add_row(["Mapped Pages", 10000, mean(m10000_mp_unmap), stdev(m10000_mp_unmap)])
table3.add_row(["VMAs", 10000, mean(m10000_vma_unmap), stdev(m10000_vma_unmap)])
table3.add_row(["Mapped Pages", 100000, mean(m100000_mp_unmap), stdev(m100000_mp_unmap)])
table3.add_row(["VMAs", 100000, mean(m100000_vma_unmap), stdev(m100000_vma_unmap)])

print(table3)
