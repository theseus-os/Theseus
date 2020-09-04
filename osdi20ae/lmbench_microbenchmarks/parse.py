from prettytable import PrettyTable

static_mean = [0,0,0,0,0]
static_std_dev = [0,0,0,0,0]

loadable_mean = [0,0,0,0,0]
loadable_std_dev =[0,0,0,0,0]

for line in open('./loadable/results.txt','r'):
    if 'BM:' in line:
        tokens = line.split()
        if 'null' in tokens:
            loadable_mean[0] = float(tokens[4])
            loadable_std_dev[0]= float(tokens[5])
        if 'ctx' in tokens:
            loadable_mean[1] = float(tokens[4])
            loadable_std_dev[1]= float(tokens[5])
        if 'spawn' in tokens:
            loadable_mean[2] = float(tokens[4])
            loadable_std_dev[2]= float(tokens[5])
        if 'mem_map' in tokens:
            loadable_mean[3] = float(tokens[4])
            loadable_std_dev[3]= float(tokens[5])
        if 'ipc' in tokens:
            loadable_mean[4] = float(tokens[4])
            loadable_std_dev[4]= float(tokens[5])

for line in open('./static/results.txt','r'):
    if 'BM:' in line:
        tokens = line.split()
        if 'null' in tokens:
            static_mean[0] = float(tokens[4])
            static_std_dev[0]= float(tokens[5])
        if 'ctx' in tokens:
            static_mean[1] = float(tokens[4])
            static_std_dev[1]= float(tokens[5])
        if 'spawn' in tokens:
            static_mean[2] = float(tokens[4])
            static_std_dev[2]= float(tokens[5])
        if 'mem_map' in tokens:
            static_mean[3] = float(tokens[4])
            static_std_dev[3]= float(tokens[5])
        if 'ipc' in tokens:
            static_mean[4] = float(tokens[4])
            static_std_dev[4]= float(tokens[5])

print("LMBench benchmark results, corresponding to Table 3. All results are in microseconds.")
table = PrettyTable()
table.field_names = ["LMBench Benchmark", "Theseus (Loadable) mean", "Theseus (Loadable) std dev ", "Theseus (Static) mean", "Theseus (Static) std dev"]
table.add_row(["null syscall", loadable_mean[0], loadable_std_dev[0], static_mean[0], static_std_dev[0]])
table.add_row(["context switch", loadable_mean[1], loadable_std_dev[1], static_mean[1], static_std_dev[1]])
table.add_row(["create process", loadable_mean[2], loadable_std_dev[2], static_mean[2], static_std_dev[2]])
table.add_row(["memory map", loadable_mean[3], loadable_std_dev[3], static_mean[3], static_std_dev[3]])
table.add_row(["IPC", loadable_mean[4], loadable_std_dev[4], static_mean[4], static_std_dev[4]])
print(table)

