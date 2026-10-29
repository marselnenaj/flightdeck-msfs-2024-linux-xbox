// SPDX-License-Identifier: MIT
// Maintainer-only builder. Never loads or executes the subject assembly.
using System.Security.Cryptography;
using Mono.Cecil;
using Mono.Cecil.Cil;

internal static class Program
{
    const string Original = "79f2bc8ddff102e6dbede84b2c211a3aea2ed675bf762fa0f02ecb65f61ee812";
    const string TypeName = "HidSharp.Platform.Windows.WinHidStream";
    static string Hash(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
    static void Check(bool value, string message) { if (!value) throw new InvalidDataException(message); }
    static MethodDefinition Method(ModuleDefinition module) => module.GetType(TypeName).Methods.Single(m =>
        m.Name == "Write" && m.Parameters.Count == 3 && m.Parameters[0].ParameterType.FullName == "System.Byte[]"
        && m.Parameters[1].ParameterType.FullName == "System.Int32" && m.Parameters[2].ParameterType.FullName == "System.Int32");
    static IEnumerable<TypeDefinition> Types(IEnumerable<TypeDefinition> types) => types.SelectMany(t => new[] { t }.Concat(Types(t.NestedTypes)));
    static string Signature(MethodDefinition method, IEnumerable<Instruction>? selected = null)
    {
        if (!method.HasBody) return method.FullName;
        var instructions = (selected ?? method.Body.Instructions).ToList();
        string Target(Instruction i) => instructions.IndexOf(i).ToString();
        string Operand(object? operand) => operand switch {
            Instruction i => Target(i), Instruction[] many => string.Join(",", many.Select(Target)),
            VariableDefinition v => $"local:{v.Index}:{v.VariableType.FullName}",
            ParameterDefinition p => $"param:{p.Index}:{p.ParameterType.FullName}",
            IMetadataTokenProvider p => p.ToString() ?? "", null => "", _ => operand.ToString() ?? ""
        };
        return method.FullName + "\n" + string.Join("\n", instructions.Select(i => i.OpCode.Name + " " + Operand(i.Operand)))
            + "\nlocals:" + string.Join(",", method.Body.Variables.Select(v => v.VariableType.FullName))
            + "\nhandlers:" + string.Join(";", method.Body.ExceptionHandlers.Select(h =>
                $"{h.HandlerType}:{(h.TryStart == null ? "-" : Target(h.TryStart))}:{(h.TryEnd == null ? "-" : Target(h.TryEnd))}:{(h.HandlerStart == null ? "-" : Target(h.HandlerStart))}:{(h.HandlerEnd == null ? "-" : Target(h.HandlerEnd))}:{(h.FilterStart == null ? "-" : Target(h.FilterStart))}:{h.CatchType?.FullName}"));
    }
    static string[] References(ModuleDefinition module) => module.AssemblyReferences.Select(r => r.FullName).ToArray();
    static int Main(string[] args)
    {
        try {
            Check(args.Length == 2, "Usage: HidPatch ORIGINAL-HidSharp.dll NEW-HidSharp.dll");
            var original = File.ReadAllBytes(args[0]);
            Check(original.Length == 239616 && Hash(original) == Original, "Only the pinned, original HidSharp 2.6.4 DLL is supported");
            Check(!File.Exists(args[1]), "Output must not already exist");
            using var input = new MemoryStream(original, writable: false);
            using var module = ModuleDefinition.ReadModule(input, new ReaderParameters { InMemory = true, ReadingMode = ReadingMode.Immediate });
            Check(module.Assembly.Name.Name == "HidSharp" && module.Assembly.Name.Version == new Version(2, 6, 4, 0), "Unexpected assembly identity");
            Check(References(module).SequenceEqual(new[] { "netstandard, Version=2.0.0.0, Culture=neutral, PublicKeyToken=cc7b13ffcd2ddd51" }), "Unexpected framework references");
            var method = Method(module);
            Check(method.Body.Instructions.Count == 143 && method.Body.CodeSize == 323 && method.Body.ExceptionHandlers.Count == 3, "Unsupported Write IL shape");
            var before = Types(module.Types).SelectMany(t => t.Methods).Select(m => Signature(m)).ToArray();
            var min = method.Body.Instructions.Select(i => i.Operand).OfType<MethodReference>().Single(m =>
                m.DeclaringType.FullName == "System.Math" && m.Name == "Min" && m.ReturnType.FullName == "System.Int32"
                && m.Parameters.Count == 2 && m.Parameters.All(p => p.ParameterType.FullName == "System.Int32"));
            Check(min.MetadataToken.ToUInt32() == 0x0a0000b7, "Expected existing Math.Min reference");
            var call = method.Body.Instructions.Single(i => i.Offset == 272 && i.OpCode == OpCodes.Call
                && i.Operand is MethodReference m && m.Name == "OverlappedOperation");
            Check(method.Body.Variables[0].VariableType.FullName == "System.UInt32"
                && method.Body.Variables[4].VariableType.FullName == "System.Int32", "Unexpected transfer locals");
            var patch = new[] {
                Instruction.Create(OpCodes.Ldloc, method.Body.Variables[4]),
                Instruction.Create(OpCodes.Ldarg, method.Parameters[2]),
                Instruction.Create(OpCodes.Call, min), // Reuse subject's netstandard MemberRef; never import host System.Math.
                Instruction.Create(OpCodes.Stloc, method.Body.Variables[0])
            };
            var il = method.Body.GetILProcessor();
            foreach (var instruction in patch) { il.InsertAfter(call, instruction); call = instruction; }
            module.Write(args[1], new WriterParameters { DeterministicMvid = true, WriteSymbols = false });
            using var check = ModuleDefinition.ReadModule(args[1], new ReaderParameters { InMemory = true, ReadingMode = ReadingMode.Immediate });
            Check(References(check).SequenceEqual(References(module)), "Framework references changed");
            var updated = Method(check);
            var inserted = updated.Body.Instructions.SkipWhile(i => !(i.OpCode == OpCodes.Call && i.Operand is MethodReference m && m.Name == "OverlappedOperation")).Skip(1).Take(4).ToArray();
            Check(inserted.Length == 4 && inserted.Select(i => i.OpCode).SequenceEqual(new[] { OpCodes.Ldloc, OpCodes.Ldarg, OpCodes.Call, OpCodes.Stloc }), "Patch verification failed");
            var after = Types(check.Types).SelectMany(t => t.Methods).Select(m => Signature(m, m == updated ? m.Body.Instructions.Where(i => !inserted.Contains(i)) : null)).ToArray();
            Check(before.SequenceEqual(after), "An unrelated method, exception handler, or instruction changed");
            Console.WriteLine($"{Hash(File.ReadAllBytes(args[1]))}  {new FileInfo(args[1]).Length} bytes; verified only Write changed; netstandard 2.0 retained");
            return 0;
        } catch (Exception error) { Console.Error.WriteLine(error.Message); return 1; }
    }
}
